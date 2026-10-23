//! Signature carving ("deep scan").
//!
//! Pipeline: one reader thread streams the device sequentially in large
//! aligned chunks (the access pattern every disk is fastest at) into a small
//! pool of recycled buffers; worker threads look for file headers in each
//! chunk and resolve every candidate's exact size from its structure.
//!
//! By default only sector starts are tested — on every filesystem a file
//! begins on a cluster (thus sector) boundary — which makes the CPU side
//! essentially free and the scan purely I/O bound. A byte-granular mode
//! (Aho-Corasick over all magics) is available for exotic cases.

pub mod formats;
pub mod window;

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};

use aho_corasick::{AhoCorasick, MatchKind};
use crossbeam_channel::bounded;

use crate::device::{read_resilient, AlignedBuf, ReadAt};
use crate::scan::{Batch, ScanCtx};
use crate::types::{Category, Condition, Extent, FileData, FoundFile, Origin};
use crate::volume::Volume;
use formats::{Ctx, Status, Stopper, SIGS};
use window::Window;

/// Extra bytes read past each chunk so headers straddling the boundary
/// validate without a device round-trip.
const TAIL: usize = 64 << 10;
/// Free ranges closer than this are read in one go (the gap is read but
/// not searched) — far fewer, larger I/Os.
const GAP_MERGE: u64 = 1 << 20;
pub(crate) const WINDOW_CACHE: usize = (1 << 20) + (16 << 10);

#[derive(Clone, Debug)]
pub struct CarveOptions {
    pub byte_level: bool,
    pub categories: u8,
    pub threads: usize,
    pub chunk: usize,
}

struct Matcher {
    byte_level: bool,
    /// (magic offset, first-byte → signature indices)
    tables: Vec<(usize, Vec<Vec<u8>>)>,
    ac: Option<AhoCorasick>,
    /// pattern id → (signature index, magic offset)
    pat_sigs: Vec<Vec<(u8, usize)>>,
    reach: usize,
}

impl Matcher {
    fn new(enabled: &[usize], byte_level: bool) -> Matcher {
        let mut tables: Vec<(usize, Vec<Vec<u8>>)> = Vec::new();
        let mut pats: Vec<&'static [u8]> = Vec::new();
        let mut pat_sigs: Vec<Vec<(u8, usize)>> = Vec::new();
        let mut reach = 0;
        for &i in enabled {
            let s = &SIGS[i];
            reach = reach.max(s.off + s.magic.len());
            if byte_level {
                match pats.iter().position(|p| *p == s.magic) {
                    Some(p) => pat_sigs[p].push((i as u8, s.off)),
                    None => {
                        pats.push(s.magic);
                        pat_sigs.push(vec![(i as u8, s.off)]);
                    }
                }
            } else {
                let t = match tables.iter().position(|t| t.0 == s.off) {
                    Some(t) => t,
                    None => {
                        tables.push((s.off, vec![Vec::new(); 256]));
                        tables.len() - 1
                    }
                };
                tables[t].1[s.magic[0] as usize].push(i as u8);
            }
        }
        let ac = byte_level.then(|| {
            AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostFirst)
                .build(&pats)
                .expect("signature automaton")
        });
        Matcher {
            byte_level,
            tables,
            ac,
            pat_sigs,
            reach,
        }
    }

    /// Candidate (offset in buf, signature) pairs inside `scan` ranges.
    fn candidates(
        &self,
        buf: &[u8],
        abs: u64,
        scan: &[(usize, usize)],
        out: &mut Vec<(usize, u8)>,
    ) {
        out.clear();
        if self.byte_level {
            let ac = self.ac.as_ref().unwrap();
            for &(a, b) in scan {
                let end = (b + self.reach).min(buf.len());
                for m in ac.find_iter(&buf[a..end]) {
                    let q = a + m.start();
                    for &(si, off) in &self.pat_sigs[m.pattern().as_usize()] {
                        if q >= a + off && q - off < b {
                            out.push((q - off, si));
                        }
                    }
                }
            }
        } else {
            for &(a, b) in scan {
                let first = (abs + a as u64).div_ceil(512) * 512 - abs;
                let mut p = first as usize;
                while p < b {
                    for (off, table) in &self.tables {
                        let Some(&byte) = buf.get(p + off) else {
                            continue;
                        };
                        for &si in &table[byte as usize] {
                            let m = SIGS[si as usize].magic;
                            if buf.get(p + off..p + off + m.len()) == Some(m) {
                                out.push((p, si));
                            }
                        }
                    }
                    p += 512;
                }
            }
        }
        out.sort_unstable();
    }
}

struct Chunk {
    abs: u64,
    buf: AlignedBuf,
    len: usize,
    scan: Vec<(usize, usize)>,
    accounted: u64,
}

/// Intervals of data already claimed by a fully-validated file.
struct Covered(Mutex<BTreeMap<u64, u64>>);

impl Covered {
    fn contains(&self, p: u64) -> bool {
        let m = self.0.lock().unwrap();
        m.range(..=p).next_back().is_some_and(|(_, &e)| e > p)
    }
    fn insert(&self, s: u64, e: u64) {
        let mut m = self.0.lock().unwrap();
        if let Some((_, &pe)) = m.range(..=s).next_back() {
            if pe >= e {
                return;
            }
        }
        let cur = m.entry(s).or_insert(e);
        *cur = (*cur).max(e);
    }
}

fn volume_at(vols: &[Volume], off: u64) -> u16 {
    vols.iter()
        .find(|v| off >= v.offset && off < v.offset + v.size)
        .map_or(u16::MAX, |v| v.index)
}

/// Carve `ranges` (absolute, sorted, merged). `covered` = byte ranges
/// already recovered by filesystem analysis, skipped here. Results stream to
/// the UI while scanning; the returned list is the final, de-duplicated set.
pub fn carve(
    src: &dyn ReadAt,
    ranges: &[(u64, u64)],
    covered: &[(u64, u64)],
    vols: &[Volume],
    opts: &CarveOptions,
    ctx: &ScanCtx,
) -> Vec<FoundFile> {
    let enabled: Vec<usize> = (0..SIGS.len())
        .filter(|&i| {
            SIGS[i].cats & opts.categories != 0 && !(opts.byte_level && SIGS[i].aligned_only)
        })
        .collect();
    let dev = src.size();
    if enabled.is_empty() || ranges.is_empty() {
        return Vec::new();
    }
    let matcher = Matcher::new(&enabled, opts.byte_level);
    let stopper = Stopper::new();

    // Read spans: ranges with small gaps merged, aligned to 4 KiB.
    let mut spans: Vec<(u64, u64)> = Vec::new();
    for &(s, l) in ranges {
        let (s, e) = (s / 4096 * 4096, ((s + l).div_ceil(4096) * 4096).min(dev));
        match spans.last_mut() {
            Some(last) if s <= last.1 + GAP_MERGE => last.1 = last.1.max(e),
            _ => spans.push((s, e)),
        }
    }
    let total: u64 = spans.iter().map(|s| s.1 - s.0).sum();
    ctx.progress.add_total(total);

    let cov = Covered(Mutex::new(BTreeMap::new()));
    for &(s, l) in covered {
        cov.insert(s, s + l);
    }
    let workers = opts.threads.clamp(1, 16);
    let chunk = opts.chunk.clamp(1 << 20, 256 << 20) / 4096 * 4096;
    let (pool_tx, pool_rx) = bounded::<AlignedBuf>(workers + 2);
    for _ in 0..workers + 2 {
        pool_tx.send(AlignedBuf::new(chunk + TAIL)).unwrap();
    }
    let (work_tx, work_rx) = bounded::<Chunk>(workers + 2);
    let results: Mutex<Vec<FoundFile>> = Mutex::new(Vec::new());

    std::thread::scope(|s| {
        let spans = &spans;
        s.spawn(move || {
            let mut ri = 0usize;
            for &(ss, se) in spans {
                let mut p = ss;
                while p < se {
                    if ctx.cancelled() {
                        return;
                    }
                    let Ok(mut buf) = pool_rx.recv() else { return };
                    let n = ((se - p) as usize).min(chunk);
                    let want = (n + TAIL).min((dev - p) as usize);
                    let (got, bad) = read_resilient(src, p, &mut buf[..want]);
                    ctx.progress.add_bad(bad);
                    let end = p + n as u64;
                    while ri < ranges.len() && ranges[ri].0 + ranges[ri].1 <= p {
                        ri += 1;
                    }
                    let mut scan = Vec::new();
                    let mut k = ri;
                    while k < ranges.len() && ranges[k].0 < end {
                        let a = ranges[k].0.max(p);
                        let b = (ranges[k].0 + ranges[k].1).min(end);
                        if b > a {
                            scan.push(((a - p) as usize, (b - p) as usize));
                        }
                        k += 1;
                    }
                    let ch = Chunk {
                        abs: p,
                        buf,
                        len: got,
                        scan,
                        accounted: n as u64,
                    };
                    if work_tx.send(ch).is_err() {
                        return;
                    }
                    p = end;
                }
            }
        });
        for _ in 0..workers {
            let (work_rx, pool_tx) = (work_rx.clone(), pool_tx.clone());
            let (matcher, stopper, cov, results) = (&matcher, &stopper, &cov, &results);
            s.spawn(move || {
                let mut cache = Some(AlignedBuf::new(WINDOW_CACHE));
                let mut local_end = 0u64;
                let mut cands = Vec::new();
                let mut mine = Vec::new();
                let mut batch = Batch::new(ctx);
                while let Ok(ch) = work_rx.recv() {
                    if !ctx.cancelled() {
                        matcher.candidates(&ch.buf[..ch.len], ch.abs, &ch.scan, &mut cands);
                        let mut last_hit = u64::MAX;
                        for &(pos, si) in &cands {
                            let abs = ch.abs + pos as u64;
                            if abs < local_end || abs == last_hit || cov.contains(abs) {
                                continue;
                            }
                            let sig = &SIGS[si as usize];
                            let w =
                                Window::new(src, abs, &ch.buf[pos..ch.len], cache.take().unwrap());
                            let mut cx = Ctx {
                                w,
                                stop: stopper,
                                limit: sig.max.min(dev - abs),
                            };
                            let r = catch_unwind(AssertUnwindSafe(|| (sig.resolve)(&mut cx)));
                            cache = Some(cx.w.into_cache());
                            let Ok(Some(mut c)) = r else { continue };
                            if c.size < 8 || c.size > sig.max {
                                continue;
                            }
                            if c.size > dev - abs {
                                c.size = dev - abs;
                                c.status = Status::Partial;
                            }
                            last_hit = abs;
                            if c.status == Status::Exact {
                                local_end = abs + c.size;
                                cov.insert(abs, abs + c.size);
                            }
                            let category = Category::from_ext(c.ext);
                            if category.bit() & opts.categories == 0 {
                                continue;
                            }
                            let name = if abs.is_multiple_of(512) {
                                format!("f{:09}.{}", abs / 512, c.ext)
                            } else {
                                format!("f{:09}_{}.{}", abs / 512, abs % 512, c.ext)
                            };
                            let f = FoundFile {
                                id: ctx.next_id(),
                                name,
                                dir: Arc::from(""),
                                ext: c.ext.to_string(),
                                category,
                                size: c.size,
                                modified: None,
                                origin: Origin::Carved,
                                condition: match c.status {
                                    Status::Exact => Condition::Good,
                                    Status::Partial => Condition::Partial,
                                    Status::Estimated => Condition::Unknown,
                                },
                                volume: volume_at(vols, abs),
                                data: FileData::Extents(vec![Extent {
                                    offset: abs,
                                    len: c.size,
                                    sparse: false,
                                }]),
                            };
                            mine.push(f.clone());
                            batch.push(f);
                        }
                    }
                    ctx.progress.add_done(ch.accounted);
                    let _ = pool_tx.send(ch.buf);
                    batch.flush();
                }
                drop(batch);
                results.lock().unwrap().extend(mine);
            });
        }
        drop(work_rx);
    });
    finalize(results.into_inner().unwrap())
}

/// Drop hits that sit inside a fully-validated file (thumbnails, embedded
/// streams, race duplicates) and trim estimated sizes at the next file.
fn finalize(mut v: Vec<FoundFile>) -> Vec<FoundFile> {
    v.sort_by_key(|f| f.first_offset());
    let mut out: Vec<FoundFile> = Vec::with_capacity(v.len());
    let mut cover_end = 0u64;
    for f in v {
        let s = f.first_offset();
        if s < cover_end {
            continue;
        }
        if f.condition == Condition::Good {
            cover_end = cover_end.max(s + f.size);
        }
        out.push(f);
    }
    for i in 0..out.len().saturating_sub(1) {
        if out[i].condition != Condition::Unknown {
            continue;
        }
        let s = out[i].first_offset();
        let next = out[i + 1].first_offset();
        if s + out[i].size > next {
            let size = next - s;
            out[i].size = size;
            if let FileData::Extents(e) = &mut out[i].data {
                e[0].len = size;
            }
        }
    }
    out
}

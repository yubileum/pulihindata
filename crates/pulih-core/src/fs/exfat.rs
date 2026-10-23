//! exFAT (SD cards, large USB sticks): deleted entry sets keep the in-use bit
//! cleared but retain the name, size and first cluster, and most files are
//! flagged "NoFatChain" (contiguous) — so recovery is usually exact.

use std::collections::HashSet;
use std::io;
use std::sync::Arc;

use super::{condition_from, count_set, emit, free_runs, FsReport};
use crate::bytes::{dos_to_unix, utf16le, Bytes};
use crate::device::{read_resilient, read_vec, ReadAt};
use crate::scan::ScanCtx;
use crate::types::{ext_of, Category, Extent, FileData, FoundFile, Origin};
use crate::volume::Volume;

struct ExFat {
    csize: u64,
    heap: u64,
    nclusters: u32,
    root: u32,
    fat: Vec<u8>,
}

impl ExFat {
    fn valid(&self, c: u32) -> bool {
        c >= 2 && c < self.nclusters + 2
    }
    fn off(&self, c: u32) -> u64 {
        self.heap + (c as u64 - 2) * self.csize
    }
    fn next(&self, c: u32) -> u32 {
        self.fat.u32le(c as usize * 4).unwrap_or(0xFFFF_FFFF)
    }
    /// Clusters of a stream: contiguous, or by FAT chain (validated).
    fn clusters(&self, first: u32, len: u64, no_chain: bool, trust_fat: bool) -> Vec<u32> {
        let n = len.div_ceil(self.csize).min(self.nclusters as u64) as usize;
        if !self.valid(first) || n == 0 {
            return Vec::new();
        }
        if !no_chain && trust_fat {
            let mut v = Vec::with_capacity(n);
            let mut c = first;
            let mut seen = HashSet::new();
            while self.valid(c) && v.len() < n && seen.insert(c) {
                v.push(c);
                c = self.next(c);
            }
            if v.len() == n {
                return v;
            }
        }
        let last = (first as u64 + n as u64).min(self.nclusters as u64 + 2) as u32;
        (first..last).collect()
    }
}

fn to_extents(fs: &ExFat, clusters: &[u32], size: u64) -> Vec<Extent> {
    let mut out: Vec<Extent> = Vec::new();
    let mut left = size;
    for &c in clusters {
        let len = fs.csize.min(left);
        if len == 0 {
            break;
        }
        left -= len;
        let off = fs.off(c);
        match out.last_mut() {
            Some(l) if l.offset + l.len == off => l.len += len,
            _ => out.push(Extent {
                offset: off,
                len,
                sparse: false,
            }),
        }
    }
    out
}

fn open(src: &dyn ReadAt, vol: &Volume) -> io::Result<ExFat> {
    let b = read_vec(src, vol.offset, 512)?;
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "exFAT: bad boot sector");
    let bps_shift = b.u8_at(108).ok_or_else(bad)? as u32;
    let spc_shift = b.u8_at(109).ok_or_else(bad)? as u32;
    if !(9..=12).contains(&bps_shift) || bps_shift + spc_shift > 25 {
        return Err(bad());
    }
    let bps = 1u64 << bps_shift;
    let fat_off = b.u32le(80).ok_or_else(bad)? as u64 * bps;
    let fat_len = b.u32le(84).ok_or_else(bad)? as u64 * bps;
    let heap = b.u32le(88).ok_or_else(bad)? as u64 * bps;
    let nclusters = b.u32le(92).ok_or_else(bad)?;
    let root = b.u32le(96).ok_or_else(bad)?;
    let fat = read_vec(
        src,
        vol.offset + fat_off,
        fat_len.min((nclusters as u64 + 2) * 4) as usize,
    )?;
    Ok(ExFat {
        csize: bps << spc_shift,
        heap: vol.offset + heap,
        nclusters,
        root,
        fat,
    })
}

fn read_stream(src: &dyn ReadAt, fs: &ExFat, clusters: &[u32], size: u64) -> Vec<u8> {
    let mut v = vec![0u8; size as usize];
    let mut pos = 0usize;
    for e in to_extents(fs, clusters, size) {
        let n = e.len as usize;
        read_resilient(src, e.offset, &mut v[pos..pos + n]);
        pos += n;
    }
    v
}

struct EntrySet {
    deleted: bool,
    attrs: u16,
    mtime: Option<i64>,
    created: u64,
    no_chain: bool,
    first: u32,
    size: u64,
    name: String,
}

/// Decode one file entry set starting at `i` (a 0x85/0x05 entry).
fn parse_set(d: &[u8], i: usize) -> Option<(EntrySet, usize)> {
    let e = d.at(i, 32)?;
    let deleted = e[0] == 0x05;
    let sc = e[1] as usize;
    if !(2..=18).contains(&sc) {
        return None;
    }
    let attrs = e.u16le(4)?;
    let ts = e.u32le(12)?;
    let mtime = dos_to_unix((ts >> 16) as u16, ts as u16);
    let cts = e.u32le(8)?;
    let created = dos_to_unix((cts >> 16) as u16, cts as u16)
        .unwrap_or(0)
        .max(0) as u64
        * 100
        + e[20] as u64;
    let s = d.at(i + 32, 32)?;
    let want = if deleted { 0x40 } else { 0xC0 };
    if s[0] != want {
        return None;
    }
    let name_len = s[3] as usize;
    let mut raw = Vec::with_capacity(sc * 30);
    for k in 2..=sc {
        let n = d.at(i + k * 32, 32)?;
        if n[0] != want + 1 {
            return None;
        }
        raw.extend_from_slice(&n[2..32]);
    }
    raw.truncate(name_len * 2);
    Some((
        EntrySet {
            deleted,
            attrs,
            mtime,
            created,
            no_chain: s[1] & 0x02 != 0,
            first: s.u32le(20)?,
            size: s.u64le(24)?,
            name: utf16le(&raw),
        },
        sc + 1,
    ))
}

pub fn scan(src: &dyn ReadAt, vol: &Volume, ctx: &ScanCtx) -> io::Result<FsReport> {
    let fs = open(src, vol)?;
    let mut report = FsReport {
        cluster: fs.csize,
        ..Default::default()
    };

    // Root directory: follow its FAT chain (always valid for live dirs).
    let root_clusters = {
        let mut v = Vec::new();
        let mut c = fs.root;
        let mut seen = HashSet::new();
        while fs.valid(c) && seen.insert(c) && v.len() < 65536 {
            v.push(c);
            c = fs.next(c);
        }
        v
    };
    let root_data = read_stream(
        src,
        &fs,
        &root_clusters,
        root_clusters.len() as u64 * fs.csize,
    );
    let mut bitmap: Option<Vec<u8>> = None;
    for e in root_data.chunks_exact(32) {
        if e[0] == 0x81 {
            let first = e.u32le(20).unwrap_or(0);
            let len = e
                .u64le(24)
                .unwrap_or(0)
                .min((fs.nclusters as u64).div_ceil(8) + 8);
            let cl = fs.clusters(first, len, false, true);
            bitmap = Some(read_stream(src, &fs, &cl, len));
            break;
        }
        if e[0] == 0 {
            break;
        }
    }
    if let Some(bm) = &bitmap {
        let mut free = Vec::new();
        free_runs(bm, fs.nclusters as u64, |s, l| {
            free.push((fs.off(s as u32 + 2), l * fs.csize))
        });
        report.free = Some(free);
    }
    if !ctx.list_files {
        return Ok(report);
    }
    ctx.set_status(format!("{}: directories", vol.desc));

    let mut out: Vec<FoundFile> = Vec::new();
    let mut keys: Vec<u64> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(fs.root);
    let mut stack: Vec<(Vec<u8>, Arc<str>, bool, u32)> = vec![(root_data, Arc::from(""), false, 0)];
    while let Some((data, path, parent_deleted, depth)) = stack.pop() {
        if ctx.cancelled() {
            break;
        }
        let mut i = 0;
        while i + 32 <= data.len() {
            let t = data[i];
            if t == 0 {
                break;
            }
            if t != 0x85 && t != 0x05 {
                i += 32;
                continue;
            }
            let Some((set, used)) = parse_set(&data, i) else {
                i += 32;
                continue;
            };
            i += used * 32;
            let deleted = set.deleted || parent_deleted;
            let is_dir = set.attrs & 0x10 != 0;
            if is_dir {
                if depth < 64
                    && fs.valid(set.first)
                    && visited.insert(set.first)
                    && set.size <= 64 << 20
                {
                    let cl = fs.clusters(set.first, set.size, set.no_chain, !deleted);
                    let sub = read_stream(src, &fs, &cl, set.size);
                    let p: Arc<str> = if path.is_empty() {
                        Arc::from(set.name.as_str())
                    } else {
                        Arc::from(format!("{}/{}", path, set.name))
                    };
                    stack.push((sub, p, deleted, depth + 1));
                }
                continue;
            }
            if !deleted || set.size == 0 || !fs.valid(set.first) {
                continue;
            }
            // Windows leaves the FAT chain of deleted files intact, so try it
            // first and fall back to contiguous clusters.
            let cl = fs.clusters(set.first, set.size, set.no_chain, true);
            let needed = set.size.div_ceil(fs.csize);
            let reused: u64 = match &bitmap {
                Some(bm) => cl.iter().map(|&c| count_set(bm, c as u64 - 2, 1)).sum(),
                None => 0,
            };
            let missing = needed.saturating_sub(cl.len() as u64);
            let ext = ext_of(&set.name);
            keys.push(set.created);
            out.push(FoundFile {
                id: ctx.next_id(),
                category: Category::from_ext(&ext),
                ext,
                name: set.name,
                dir: path.clone(),
                size: set.size,
                modified: set.mtime,
                origin: Origin::ExFat,
                condition: condition_from(reused + missing, needed),
                volume: vol.index,
                data: FileData::Extents(to_extents(&fs, &cl, set.size)),
                info: None,
            });
        }
    }
    report.found = emit(ctx, out, &keys);
    Ok(report)
}

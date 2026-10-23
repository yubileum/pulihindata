//! NTFS: walk the whole MFT and report records that are no longer in use.
//!
//! A deleted file's MFT record keeps its name, parent folder, timestamps and
//! data runlist until the record is reused, so fragmented files are
//! recovered exactly. `$Bitmap` tells us whether the clusters were reused.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use super::{condition_from, count_set, emit, free_runs, FsReport};
use crate::bytes::{filetime_to_unix, utf16le, Bytes};
use crate::device::{read_full, read_vec, ReadAt};
use crate::scan::ScanCtx;
use crate::types::{ext_of, Category, Condition, Extent, FileData, FoundFile, Origin};
use crate::volume::Volume;

const ROOT_RECORD: u64 = 5;

#[derive(Clone, Debug)]
struct Geometry {
    base: u64,
    cluster: u64,
    rsize: usize,
    total_clusters: u64,
}

#[derive(Clone, Copy, Debug)]
struct Run {
    /// `None` = sparse.
    lcn: Option<u64>,
    len: u64,
}

#[derive(Debug)]
enum Data {
    Resident(Vec<u8>),
    NonResident {
        start_vcn: u64,
        runs: Vec<Run>,
        real_size: u64,
        flags: u16,
    },
}

#[derive(Default, Debug)]
struct Record {
    in_use: bool,
    is_dir: bool,
    seq: u16,
    base_ref: u64,
    /// (parent ref, name, namespace)
    name: Option<(u64, String, u8)>,
    si_mtime: Option<i64>,
    si_ctime: u64,
    fn_mtime: Option<i64>,
    data: Vec<Data>,
    attr_list: Option<Data>,
}

/// Apply the update-sequence fixups that protect multi-sector records.
fn fixup(rec: &mut [u8]) -> bool {
    let (Some(off), Some(cnt)) = (rec.u16le(4), rec.u16le(6)) else {
        return false;
    };
    let (off, cnt) = (off as usize, cnt as usize);
    if cnt < 2 || off + cnt * 2 > rec.len() {
        return false;
    }
    let stride = rec.len() / (cnt - 1);
    if stride < 256 {
        return false;
    }
    let usn = [rec[off], rec[off + 1]];
    for i in 1..cnt {
        let end = i * stride - 2;
        if rec[end..end + 2] != usn {
            return false;
        }
        rec[end] = rec[off + i * 2];
        rec[end + 1] = rec[off + i * 2 + 1];
    }
    true
}

fn decode_runs(b: &[u8]) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let mut pos = 0usize;
    let mut lcn: i64 = 0;
    while let Some(&h) = b.get(pos) {
        if h == 0 {
            break;
        }
        let (ls, os) = ((h & 0xF) as usize, (h >> 4) as usize);
        if ls == 0 || ls > 8 || os > 8 {
            return None;
        }
        pos += 1;
        let lb = b.at(pos, ls)?;
        let mut len = 0u64;
        for (i, &x) in lb.iter().enumerate() {
            len |= (x as u64) << (8 * i);
        }
        pos += ls;
        if os == 0 {
            runs.push(Run { lcn: None, len });
        } else {
            let ob = b.at(pos, os)?;
            let mut d = 0i64;
            for (i, &x) in ob.iter().enumerate() {
                d |= (x as i64) << (8 * i);
            }
            // sign-extend
            let shift = 64 - 8 * os as u32;
            d = (d << shift) >> shift;
            lcn = lcn.checked_add(d)?;
            if lcn < 0 {
                return None;
            }
            runs.push(Run {
                lcn: Some(lcn as u64),
                len,
            });
        }
        pos += os;
        if runs.len() > 1_000_000 {
            return None;
        }
    }
    Some(runs)
}

/// Parse a (fixed-up) MFT record. `full` = also decode names/data
/// (otherwise only the header flags are filled).
fn parse_record(rec: &[u8], full: bool) -> Option<Record> {
    if rec.get(0..4)? != b"FILE" {
        return None;
    }
    let flags = rec.u16le(0x16)?;
    let mut r = Record {
        in_use: flags & 1 != 0,
        is_dir: flags & 2 != 0,
        seq: rec.u16le(0x10)?,
        base_ref: rec.u64le(0x20)? & 0x0000_FFFF_FFFF_FFFF,
        ..Default::default()
    };
    if !full {
        return Some(r);
    }
    let used = (rec.u32le(0x18)? as usize).min(rec.len());
    let mut pos = rec.u16le(0x14)? as usize;
    for _ in 0..64 {
        let ty = rec.u32le(pos)?;
        if ty == 0xFFFF_FFFF {
            break;
        }
        let len = rec.u32le(pos + 4)? as usize;
        if len < 16 || pos + len > used {
            break;
        }
        let a = &rec[pos..pos + len];
        let nonres = a[8] != 0;
        let name_len = a[9];
        let aflags = a.u16le(12)?;
        match (ty, nonres) {
            (0x10, false) => {
                let v = resident_value(a)?;
                r.si_ctime = v.u64le(0).unwrap_or(0);
                r.si_mtime = v
                    .u64le(8)
                    .and_then(filetime_to_unix)
                    .map(|t| t + crate::bytes::local_offset());
            }
            (0x30, false) => {
                let v = resident_value(a)?;
                let parent = v.u64le(0)?;
                let nlen = *v.get(64)? as usize;
                let ns = *v.get(65)?;
                let name = utf16le(v.at(66, nlen * 2)?);
                // Prefer Win32 / POSIX names over the 8.3 DOS alias.
                let better = match &r.name {
                    None => true,
                    Some((_, _, old)) => *old == 2 && ns != 2,
                };
                if better {
                    r.fn_mtime = v
                        .u64le(16)
                        .and_then(filetime_to_unix)
                        .map(|t| t + crate::bytes::local_offset());
                    r.name = Some((parent, name, ns));
                }
            }
            (0x20, _) => {
                r.attr_list = Some(if nonres {
                    nonres_data(a, aflags)?
                } else {
                    Data::Resident(resident_value(a)?.to_vec())
                });
            }
            (0x80, _) if name_len == 0 => {
                r.data.push(if nonres {
                    nonres_data(a, aflags)?
                } else {
                    Data::Resident(resident_value(a)?.to_vec())
                });
            }
            _ => {}
        }
        pos += len;
    }
    Some(r)
}

fn resident_value(a: &[u8]) -> Option<&[u8]> {
    let vlen = a.u32le(16)? as usize;
    let voff = a.u16le(20)? as usize;
    a.at(voff, vlen)
}

fn nonres_data(a: &[u8], flags: u16) -> Option<Data> {
    let start_vcn = a.u64le(16)?;
    let roff = a.u16le(32)? as usize;
    let real_size = a.u64le(48)?;
    let runs = decode_runs(a.get(roff..)?)?;
    Some(Data::NonResident {
        start_vcn,
        runs,
        real_size,
        flags,
    })
}

fn runs_to_extents(g: &Geometry, runs: &[Run], size: u64) -> Vec<Extent> {
    let mut out = Vec::with_capacity(runs.len());
    let mut left = size;
    for r in runs {
        if left == 0 {
            break;
        }
        let len = (r.len * g.cluster).min(left);
        left -= len;
        match r.lcn {
            Some(lcn) if lcn + r.len <= g.total_clusters + 1 => out.push(Extent {
                offset: g.base + lcn * g.cluster,
                len,
                sparse: false,
            }),
            Some(_) => return Vec::new(), // runlist points outside the volume: garbage
            None => out.push(Extent {
                offset: 0,
                len,
                sparse: true,
            }),
        }
    }
    if left > 0 {
        // Runlist shorter than the file (tail lost): still return what exists.
        out.retain(|e| e.len > 0);
    }
    out
}

/// Read a non-resident attribute's full content.
fn read_runs(src: &dyn ReadAt, g: &Geometry, runs: &[Run], size: u64) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; size as usize];
    let mut pos = 0usize;
    for e in runs_to_extents(g, runs, size) {
        let n = e.len as usize;
        if !e.sparse {
            read_full(src, e.offset, &mut out[pos..pos + n])?;
        }
        pos += n;
    }
    Ok(out)
}

struct Mft {
    g: Geometry,
    /// MFT extents as (absolute byte offset, byte length).
    extents: Vec<(u64, u64)>,
    records: u64,
}

impl Mft {
    fn record_offset(&self, no: u64) -> Option<u64> {
        let mut byte = no * self.g.rsize as u64;
        for &(off, len) in &self.extents {
            if byte < len {
                return Some(off + byte);
            }
            byte -= len;
        }
        None
    }

    fn read_record(&self, src: &dyn ReadAt, no: u64) -> Option<Record> {
        let off = self.record_offset(no)?;
        let mut b = read_vec(src, off, self.g.rsize).ok()?;
        if b.len() != self.g.rsize || !fixup(&mut b) {
            return None;
        }
        parse_record(&b, true)
    }
}

fn open(src: &dyn ReadAt, vol: &Volume) -> io::Result<Mft> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, format!("NTFS: {m}"));
    let b = read_vec(src, vol.offset, 512)?;
    let bps = b.u16le(11).unwrap_or(0) as u64;
    let spc_raw = b.u8_at(13).unwrap_or(0);
    let spc = if spc_raw <= 0x80 {
        spc_raw as u64
    } else {
        1u64 << (256 - spc_raw as u32).min(31)
    };
    let cluster = bps * spc;
    if !matches!(bps, 512 | 1024 | 2048 | 4096) || cluster == 0 || cluster > 2 << 20 {
        return Err(bad("bad boot sector geometry"));
    }
    let total_sectors = b.u64le(0x28).unwrap_or(0);
    let mft_lcn = b.u64le(0x30).unwrap_or(0);
    let cpr = b.u8_at(0x40).unwrap_or(0) as i8;
    let rsize = if cpr > 0 {
        cpr as u64 * cluster
    } else {
        1u64 << (-(cpr as i32)).clamp(0, 20)
    };
    if !rsize.is_power_of_two() || !(256..=65536).contains(&rsize) {
        return Err(bad("bad MFT record size"));
    }
    let g = Geometry {
        base: vol.offset,
        cluster,
        rsize: rsize as usize,
        total_clusters: total_sectors / spc,
    };
    let mut rec0 = read_vec(src, vol.offset + mft_lcn * cluster, g.rsize)?;
    if rec0.len() != g.rsize || !fixup(&mut rec0) {
        return Err(bad("$MFT record unreadable"));
    }
    let r0 = parse_record(&rec0, true).ok_or_else(|| bad("$MFT record invalid"))?;
    let mut frags: Vec<(u64, Vec<Run>, u64)> = Vec::new();
    for d in r0.data {
        if let Data::NonResident {
            start_vcn,
            runs,
            real_size,
            ..
        } = d
        {
            frags.push((start_vcn, runs, real_size));
        }
    }
    let mut mft = Mft {
        g: g.clone(),
        extents: Vec::new(),
        records: 0,
    };
    let rebuild = |mft: &mut Mft, frags: &mut Vec<(u64, Vec<Run>, u64)>| {
        frags.sort_by_key(|f| f.0);
        let size = frags.iter().find(|f| f.0 == 0).map_or(0, |f| f.2);
        let runs: Vec<Run> = frags.iter().flat_map(|f| f.1.iter().copied()).collect();
        mft.extents = runs_to_extents(&g, &runs, size)
            .into_iter()
            .filter(|e| !e.sparse)
            .map(|e| (e.offset, e.len))
            .collect();
        mft.records = size / g.rsize as u64;
    };
    rebuild(&mut mft, &mut frags);
    // A heavily fragmented $MFT continues in extension records listed in
    // its $ATTRIBUTE_LIST.
    if let Some(al) = r0.attr_list {
        let list = match al {
            Data::Resident(v) => v,
            Data::NonResident {
                runs, real_size, ..
            } => read_runs(src, &g, &runs, real_size.min(1 << 24))?,
        };
        let mut pos = 0;
        let mut seen = std::collections::HashSet::new();
        while let (Some(ty), Some(len)) = (list.u32le(pos), list.u16le(pos + 4)) {
            if len < 26 {
                break;
            }
            let rec_no = list.u64le(pos + 16).unwrap_or(0) & 0x0000_FFFF_FFFF_FFFF;
            if ty == 0x80 && rec_no != 0 && seen.insert(rec_no) {
                if let Some(ext) = mft.read_record(src, rec_no) {
                    for d in ext.data {
                        if let Data::NonResident {
                            start_vcn,
                            runs,
                            real_size,
                            ..
                        } = d
                        {
                            if start_vcn != 0 {
                                frags.push((start_vcn, runs, real_size));
                            }
                        }
                    }
                }
            }
            pos += len as usize;
        }
        rebuild(&mut mft, &mut frags);
    }
    if mft.records == 0 {
        return Err(bad("empty MFT"));
    }
    Ok(mft)
}

fn load_bitmap(src: &dyn ReadAt, mft: &Mft) -> Option<Vec<u8>> {
    let r = mft.read_record(src, 6)?;
    match r.data.into_iter().next()? {
        Data::Resident(v) => Some(v),
        Data::NonResident {
            runs, real_size, ..
        } => {
            let need = mft.g.total_clusters.div_ceil(8);
            read_runs(src, &mft.g, &runs, real_size.min(need.max(1) + 4096)).ok()
        }
    }
}

struct DirEntry {
    parent: u64,
    name: String,
    seq: u16,
    in_use: bool,
}

struct Deleted {
    rec_no: u64,
    rec: Record,
}

pub fn scan(src: &dyn ReadAt, vol: &Volume, ctx: &ScanCtx) -> io::Result<FsReport> {
    let mft = open(src, vol)?;
    let g = mft.g.clone();
    let bitmap = load_bitmap(src, &mft);
    let mut report = FsReport {
        cluster: g.cluster,
        ..Default::default()
    };
    if let Some(bm) = &bitmap {
        let mut free = Vec::new();
        free_runs(bm, g.total_clusters, |s, l| {
            free.push((g.base + s * g.cluster, l * g.cluster));
        });
        report.free = Some(free);
    }
    if !ctx.list_files {
        return Ok(report);
    }

    ctx.set_status(format!("{}: MFT ({} records)", vol.desc, mft.records));
    let total_bytes = mft.records * g.rsize as u64;
    ctx.progress.add_total(total_bytes);

    let mut dirs: HashMap<u64, DirEntry> = HashMap::new();
    let mut deleted: Vec<Deleted> = Vec::new();
    // Extension-record $DATA fragments of deleted files, keyed by base record.
    let mut ext_frags: HashMap<u64, Vec<Data>> = HashMap::new();

    const CHUNK: usize = 8 << 20;
    let mut buf = vec![0u8; CHUNK];
    let mut rec_no = 0u64;
    'outer: for &(ext_off, ext_len) in &mft.extents {
        let mut done = 0u64;
        while done < ext_len {
            if ctx.cancelled() {
                break 'outer;
            }
            let n = ((ext_len - done) as usize).min(CHUNK) / g.rsize * g.rsize;
            if n == 0 {
                break;
            }
            let (got, bad) = crate::device::read_resilient(src, ext_off + done, &mut buf[..n]);
            ctx.progress.add_bad(bad);
            for rec in buf[..got].chunks_exact_mut(g.rsize) {
                if rec_no >= mft.records {
                    break 'outer;
                }
                let no = rec_no;
                rec_no += 1;
                // Cheap pre-check before the fixup: only free records and
                // directories are interesting.
                if &rec[0..4] != b"FILE" {
                    continue;
                }
                let flags = rec.u16le(0x16).unwrap_or(1);
                let in_use = flags & 1 != 0;
                if in_use && flags & 2 == 0 {
                    continue;
                }
                if !fixup(rec) {
                    continue;
                }
                let Some(r) = parse_record(rec, true) else {
                    continue;
                };
                if r.base_ref != 0 {
                    if !r.in_use && !r.data.is_empty() {
                        ext_frags.entry(r.base_ref).or_default().extend(r.data);
                    }
                    continue;
                }
                if r.is_dir {
                    if let Some((parent, name, _)) = &r.name {
                        dirs.insert(
                            no,
                            DirEntry {
                                parent: *parent,
                                name: name.clone(),
                                seq: r.seq,
                                in_use,
                            },
                        );
                    }
                } else if !r.in_use && r.name.is_some() {
                    deleted.push(Deleted { rec_no: no, rec: r });
                }
            }
            done += n as u64;
            ctx.progress.add_done(n as u64);
        }
    }

    // Path reconstruction with memoisation.
    let mut paths: HashMap<u64, Arc<str>> = HashMap::new();
    let orphan: Arc<str> = Arc::from("$Orphan");
    let mut resolve = |parent_ref: u64| -> Arc<str> {
        let mut chain: Vec<u64> = Vec::new();
        let mut cur = parent_ref;
        let mut prefix: Option<Arc<str>> = None;
        for _ in 0..512 {
            let no = cur & 0x0000_FFFF_FFFF_FFFF;
            let seq = (cur >> 48) as u16;
            if no == ROOT_RECORD {
                prefix = Some(Arc::from(""));
                break;
            }
            if let Some(p) = paths.get(&no) {
                prefix = Some(p.clone());
                break;
            }
            let Some(d) = dirs.get(&no) else { break };
            // A deleted record's sequence number was bumped on deletion.
            let seq_ok = seq == 0 || seq == d.seq || (!d.in_use && seq.wrapping_add(1) == d.seq);
            if !seq_ok || chain.contains(&no) {
                break;
            }
            chain.push(no);
            cur = d.parent;
        }
        let mut p = prefix.unwrap_or_else(|| orphan.clone());
        for &no in chain.iter().rev() {
            let name = &dirs[&no].name;
            let joined: Arc<str> = if p.is_empty() {
                Arc::from(name.as_str())
            } else {
                Arc::from(format!("{p}/{name}"))
            };
            paths.insert(no, joined.clone());
            p = joined;
        }
        p
    };

    let mut out: Vec<FoundFile> = Vec::new();
    let mut keys: Vec<u64> = Vec::new();
    let mut skipped_compressed = 0usize;
    for d in deleted {
        let Some((parent, name, _)) = &d.rec.name else {
            continue;
        };
        let mut frags: Vec<Data> = d.rec.data;
        if let Some(more) = ext_frags.remove(&d.rec_no) {
            frags.extend(more);
        }
        let (data, size, compressed) = match frags.len() {
            0 => continue,
            _ => {
                if let Some(pos) = frags.iter().position(|f| matches!(f, Data::Resident(_))) {
                    let Data::Resident(v) = frags.swap_remove(pos) else {
                        unreachable!()
                    };
                    let n = v.len() as u64;
                    (FileData::Resident(Arc::from(v)), n, false)
                } else {
                    let mut parts: Vec<(u64, Vec<Run>, u64, u16)> = frags
                        .into_iter()
                        .filter_map(|f| match f {
                            Data::NonResident {
                                start_vcn,
                                runs,
                                real_size,
                                flags,
                            } => Some((start_vcn, runs, real_size, flags)),
                            _ => None,
                        })
                        .collect();
                    parts.sort_by_key(|p| p.0);
                    let size = parts.iter().find(|p| p.0 == 0).map_or(0, |p| p.2);
                    let flags = parts.first().map_or(0, |p| p.3);
                    let runs: Vec<Run> = parts.into_iter().flat_map(|p| p.1).collect();
                    let ext = runs_to_extents(&g, &runs, size);
                    (FileData::Extents(ext), size, flags & 0x4001 != 0)
                }
            }
        };
        if size == 0 {
            continue;
        }
        if compressed {
            // NTFS-compressed / EFS-encrypted data cannot be restored raw.
            skipped_compressed += 1;
            continue;
        }
        let condition = match (&data, &bitmap) {
            (FileData::Resident(_), _) => Condition::Good,
            (FileData::Extents(e), _) if e.is_empty() => continue,
            (FileData::Extents(e), Some(bm)) => {
                let mut total = 0;
                let mut reused = 0;
                for x in e.iter().filter(|x| !x.sparse) {
                    let lcn = (x.offset - g.base) / g.cluster;
                    let n = x.len.div_ceil(g.cluster);
                    total += n;
                    reused += count_set(bm, lcn, n);
                }
                condition_from(reused, total)
            }
            _ => Condition::Unknown,
        };
        let ext = ext_of(name);
        keys.push(d.rec.si_ctime);
        out.push(FoundFile {
            id: ctx.next_id(),
            category: Category::from_ext(&ext),
            ext,
            name: name.clone(),
            dir: resolve(*parent),
            size,
            modified: d.rec.si_mtime.or(d.rec.fn_mtime),
            origin: Origin::Ntfs,
            condition,
            volume: vol.index,
            data,
            info: None,
        });
    }
    report.found = emit(ctx, out, &keys);
    if skipped_compressed > 0 {
        ctx.log(format!(
            "{}: {skipped_compressed} compressed/encrypted deleted files skipped",
            vol.desc
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runlist() {
        // 0x21: 1 length byte, 2 offset bytes. len 0x18 @ lcn 0x5634,
        // then len 0x10 @ -0x100 relative, then sparse len 4.
        let b = [
            0x21, 0x18, 0x34, 0x56, 0x21, 0x10, 0x00, 0xFF, 0x01, 0x04, 0x00,
        ];
        let r = decode_runs(&b).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].lcn, r[0].len), (Some(0x5634), 0x18));
        assert_eq!((r[1].lcn, r[1].len), (Some(0x5534), 0x10));
        assert_eq!((r[2].lcn, r[2].len), (None, 4));
    }
}

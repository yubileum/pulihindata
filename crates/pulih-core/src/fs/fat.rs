//! FAT12/16/32: walk every directory (including deleted ones) and report
//! entries whose first byte was replaced by 0xE5.
//!
//! Deleting a file on FAT zeroes its cluster chain, so data is assumed to be
//! contiguous from the start cluster — true for the vast majority of files
//! on cameras, phones and USB sticks. Long names are rebuilt from the
//! (also deleted) LFN entries in front of the short entry.

use std::collections::HashSet;
use std::io;
use std::sync::Arc;

use super::{condition_from, emit, FsReport};
use crate::bytes::{dos_to_unix, utf16le, Bytes};
use crate::device::{read_resilient, read_vec, ReadAt};
use crate::scan::ScanCtx;
use crate::types::{ext_of, Category, Extent, FileData, FoundFile, Origin};
use crate::volume::{fat_kind, FsType, Volume};

struct Fat {
    kind: FsType,
    csize: u64,
    data_off: u64,
    root_off: u64,
    root_len: u64,
    root_cluster: u32,
    nclusters: u32,
    table: Vec<u8>,
}

impl Fat {
    fn entry(&self, n: u32) -> u32 {
        let t = &self.table;
        match self.kind {
            FsType::Fat12 => {
                let o = n as usize * 3 / 2;
                let v = t.u16le(o).unwrap_or(0xFFF) as u32;
                if n & 1 == 0 {
                    v & 0xFFF
                } else {
                    v >> 4
                }
            }
            FsType::Fat16 => t.u16le(n as usize * 2).unwrap_or(0xFFFF) as u32,
            _ => t.u32le(n as usize * 4).unwrap_or(0x0FFF_FFFF) & 0x0FFF_FFFF,
        }
    }

    fn eoc(&self) -> u32 {
        match self.kind {
            FsType::Fat12 => 0xFF8,
            FsType::Fat16 => 0xFFF8,
            _ => 0x0FFF_FFF8,
        }
    }

    fn valid(&self, c: u32) -> bool {
        c >= 2 && c < self.nclusters + 2
    }

    fn cluster_off(&self, c: u32) -> u64 {
        self.data_off + (c as u64 - 2) * self.csize
    }

    fn chain(&self, start: u32, limit: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut c = start;
        let mut seen = HashSet::new();
        while self.valid(c) && out.len() < limit && seen.insert(c) {
            out.push(c);
            let next = self.entry(c);
            if next >= self.eoc() || next == 0 {
                break;
            }
            c = next;
        }
        out
    }
}

fn open(src: &dyn ReadAt, vol: &Volume) -> io::Result<Fat> {
    let b = read_vec(src, vol.offset, 512)?;
    let kind = fat_kind(&b)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "FAT: bad boot sector"))?;
    let bps = b.u16le(11).unwrap() as u64;
    let spc = b.u8_at(13).unwrap() as u64;
    let reserved = b.u16le(14).unwrap() as u64;
    let nfats = b.u8_at(16).unwrap() as u64;
    let root_ents = b.u16le(17).unwrap() as u64;
    let tot16 = b.u16le(19).unwrap() as u64;
    let tot = if tot16 != 0 {
        tot16
    } else {
        b.u32le(32).unwrap() as u64
    };
    let fsz16 = b.u16le(22).unwrap() as u64;
    let fsz = if fsz16 != 0 {
        fsz16
    } else {
        b.u32le(36).unwrap() as u64
    };
    let root_secs = (root_ents * 32).div_ceil(bps);
    let fat_off = vol.offset + reserved * bps;
    let root_off = fat_off + nfats * fsz * bps;
    let data_off = root_off + root_secs * bps;
    let data_start = reserved + nfats * fsz + root_secs;
    let nclusters = ((tot - data_start) / spc) as u32;
    let table_len = match kind {
        FsType::Fat12 => (nclusters as u64 + 2) * 3 / 2 + 2,
        FsType::Fat16 => (nclusters as u64 + 2) * 2,
        _ => (nclusters as u64 + 2) * 4,
    }
    .min(fsz * bps);
    let table = read_vec(src, fat_off, table_len as usize)?;
    Ok(Fat {
        kind,
        csize: spc * bps,
        data_off,
        root_off,
        root_len: root_ents * 32,
        root_cluster: if kind == FsType::Fat32 {
            b.u32le(44).unwrap_or(2)
        } else {
            0
        },
        nclusters,
        table,
    })
}

fn lfn_checksum(sfn: &[u8]) -> u8 {
    sfn.iter().fold(0u8, |s, &c| {
        ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c)
    })
}

fn short_name(e: &[u8], first: u8) -> String {
    let mut base: Vec<u8> = e[0..8].to_vec();
    base[0] = first;
    let base = String::from_utf8_lossy(&base).trim_end().to_string();
    let ext = String::from_utf8_lossy(&e[8..11]).trim_end().to_string();
    let lower_base = e[12] & 0x08 != 0;
    let lower_ext = e[12] & 0x10 != 0;
    let base = if lower_base {
        base.to_lowercase()
    } else {
        base
    };
    let ext = if lower_ext { ext.to_lowercase() } else { ext };
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

/// Some Windows versions zero the high 16 bits of a FAT32 start cluster on
/// deletion. If the low-word cluster is unusable, look for a free run at
/// `(h << 16) | lo` whose first bytes match the file type.
fn guess_high_word(src: &dyn ReadAt, fat: &Fat, lo: u32, size: u64, ext: &str) -> Option<u32> {
    let usable = |c: u32| fat.valid(c) && fat.entry(c) == 0;
    if usable(lo) || fat.nclusters < 0x1_0000 {
        return None;
    }
    let need = size.div_ceil(fat.csize).min(16) as u32;
    let magic = crate::carve::formats::magic_for_ext(ext);
    let mut free_candidates = Vec::new();
    for h in 1..=((fat.nclusters + 1) >> 16) {
        let c = (h << 16) | lo;
        if !(0..need).all(|i| usable(c + i)) {
            continue;
        }
        if let Some((m, off)) = magic {
            let head = read_vec(src, fat.cluster_off(c), off + m.len()).ok()?;
            if head.get(off..off + m.len()) == Some(m) {
                return Some(c);
            }
        }
        free_candidates.push(c);
    }
    (magic.is_none() && free_candidates.len() == 1).then(|| free_candidates[0])
}

enum Loc {
    Root16,
    Live(u32),
    Deleted(u32),
}

pub fn scan(src: &dyn ReadAt, vol: &Volume, ctx: &ScanCtx) -> io::Result<FsReport> {
    let fat = open(src, vol)?;
    let mut report = FsReport {
        cluster: fat.csize,
        ..Default::default()
    };
    let mut free = Vec::new();
    let mut run: Option<u32> = None;
    for c in 2..fat.nclusters + 2 {
        let is_free = fat.entry(c) == 0;
        match (is_free, run) {
            (true, None) => run = Some(c),
            (false, Some(s)) => {
                free.push((fat.cluster_off(s), (c - s) as u64 * fat.csize));
                run = None;
            }
            _ => {}
        }
    }
    if let Some(s) = run {
        free.push((
            fat.cluster_off(s),
            (fat.nclusters + 2 - s) as u64 * fat.csize,
        ));
    }
    report.free = Some(free);
    if !ctx.list_files {
        return Ok(report);
    }
    ctx.set_status(format!("{}: directories", vol.desc));

    let mut out: Vec<FoundFile> = Vec::new();
    let mut keys: Vec<u64> = Vec::new();
    let mut visited: HashSet<u32> = HashSet::new();
    let root = if fat.kind == FsType::Fat32 {
        Loc::Live(fat.root_cluster)
    } else {
        Loc::Root16
    };
    let mut stack: Vec<(Loc, Arc<str>, u32)> = vec![(root, Arc::from(""), 0)];
    let max_dir_clusters = ((4u64 << 20) / fat.csize).max(1) as usize; // 4 MB of entries

    while let Some((loc, path, depth)) = stack.pop() {
        if ctx.cancelled() {
            break;
        }
        let in_deleted = matches!(loc, Loc::Deleted(_));
        let data = match loc {
            Loc::Root16 => read_vec(src, fat.root_off, fat.root_len as usize)?,
            Loc::Live(c) => {
                let chain = fat.chain(c, max_dir_clusters);
                let mut v = Vec::with_capacity(chain.len() * fat.csize as usize);
                for c in chain {
                    if !visited.insert(c) {
                        break;
                    }
                    let mut b = vec![0u8; fat.csize as usize];
                    read_resilient(src, fat.cluster_off(c), &mut b);
                    v.extend_from_slice(&b);
                }
                v
            }
            Loc::Deleted(c) => {
                // Chain is gone: read contiguous clusters until the
                // end-of-directory marker shows up.
                let mut v = Vec::new();
                let mut cur = c;
                while fat.valid(cur) && v.len() < max_dir_clusters * fat.csize as usize {
                    if !visited.insert(cur) {
                        break;
                    }
                    let mut b = vec![0u8; fat.csize as usize];
                    read_resilient(src, fat.cluster_off(cur), &mut b);
                    let ended = b.chunks_exact(32).any(|e| e[0] == 0);
                    v.extend_from_slice(&b);
                    if ended {
                        break;
                    }
                    cur += 1;
                }
                // The directory must still start with "." / ".." — otherwise
                // its cluster has been reused.
                if v.len() < 64 || &v[0..2] != b". " || v[11] & 0x10 == 0 {
                    continue;
                }
                v
            }
        };

        let mut lfn: Vec<(u8, String)> = Vec::new(); // (checksum, 13-char piece)
        for e in data.chunks_exact(32) {
            let first = e[0];
            if first == 0 {
                break;
            }
            let attr = e[11];
            if attr == 0x0F {
                let mut raw = Vec::with_capacity(26);
                raw.extend_from_slice(&e[1..11]);
                raw.extend_from_slice(&e[14..26]);
                raw.extend_from_slice(&e[28..32]);
                // 0xFFFF padding after the terminating NUL.
                let piece: String = utf16le(&raw).chars().filter(|&c| c != '\u{FFFF}').collect();
                if first != 0xE5 && first & 0x40 != 0 {
                    lfn.clear();
                }
                lfn.push((e[13], piece));
                continue;
            }
            let pieces = std::mem::take(&mut lfn);
            if attr & 0x08 != 0 || &e[0..2] == b". " || &e[0..3] == b".. " {
                continue;
            }
            let deleted = first == 0xE5 || in_deleted;
            let is_dir = attr & 0x10 != 0;
            if !deleted && !is_dir {
                continue;
            }
            let hi = if fat.kind == FsType::Fat32 {
                e.u16le(20).unwrap_or(0) as u32
            } else {
                0
            };
            let lo = e.u16le(26).unwrap_or(0) as u32;
            let mut cluster = (hi << 16) | lo;
            let size = e.u32le(28).unwrap_or(0) as u64;

            // Long name: pieces were stored last-piece-first.
            let mut name = String::new();
            if !pieces.is_empty() && pieces.iter().all(|p| p.0 == pieces[0].0) {
                let long: String = pieces.iter().rev().map(|p| p.1.as_str()).collect();
                let want = pieces[0].0;
                let ok = if first == 0xE5 {
                    // The lost first byte is the one value that satisfies the
                    // checksum; it must be a legal short-name character.
                    let mut sfn = e[0..11].to_vec();
                    (0x21u8..=0xFF).any(|b| {
                        sfn[0] = b;
                        b != 0xE5
                            && !b"\"*+,./:;<=>?[\\]|\x7F".contains(&b)
                            && lfn_checksum(&sfn) == want
                    })
                } else {
                    lfn_checksum(&e[0..11]) == want
                };
                if ok && !long.is_empty() {
                    name = long;
                }
            }
            if name.is_empty() {
                let first_char = if first == 0xE5 {
                    b'_'
                } else if first == 0x05 {
                    0xE5
                } else {
                    first
                };
                name = short_name(e, first_char);
            }

            if is_dir {
                if fat.valid(cluster) && depth < 64 {
                    let sub: Arc<str> = if path.is_empty() {
                        Arc::from(name.as_str())
                    } else {
                        Arc::from(format!("{path}/{name}"))
                    };
                    let loc = if deleted {
                        Loc::Deleted(cluster)
                    } else {
                        Loc::Live(cluster)
                    };
                    stack.push((loc, sub, depth + 1));
                }
                continue;
            }
            if deleted && hi == 0 && fat.kind == FsType::Fat32 && size > 0 {
                if let Some(c) = guess_high_word(src, &fat, lo, size, &ext_of(&name)) {
                    cluster = c;
                }
            }
            if size == 0 || !fat.valid(cluster) {
                continue;
            }
            let n = size.div_ceil(fat.csize);
            let avail = (fat.nclusters as u64 + 2 - cluster as u64).min(n);
            if avail == 0 {
                continue;
            }
            let reused = (cluster..cluster + avail as u32)
                .filter(|&c| fat.entry(c) != 0)
                .count() as u64;
            let len = (avail * fat.csize).min(size);
            let ext = ext_of(&name);
            let created =
                dos_to_unix(e.u16le(16).unwrap_or(0), e.u16le(14).unwrap_or(0)).unwrap_or(0);
            keys.push(created.max(0) as u64 * 100 + e[13] as u64);
            out.push(FoundFile {
                id: ctx.next_id(),
                category: Category::from_ext(&ext),
                ext,
                name,
                dir: path.clone(),
                size,
                modified: dos_to_unix(e.u16le(24).unwrap_or(0), e.u16le(22).unwrap_or(0)),
                origin: Origin::Fat,
                condition: condition_from(reused + (n - avail), n),
                volume: vol.index,
                data: FileData::Extents(vec![Extent {
                    offset: fat.cluster_off(cluster),
                    len,
                    sparse: false,
                }]),
                info: None,
            });
        }
    }
    report.found = emit(ctx, out, &keys);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum() {
        assert_eq!(lfn_checksum(b"README  TXT"), 0x73);
    }
}

//! Filesystem-aware recovery: reads deleted entries straight from the
//! filesystem metadata, so results keep their names, folders and dates and
//! fragmented files come back whole.

pub mod exfat;
pub mod fat;
pub mod ntfs;

use crate::scan::{Batch, ScanCtx};
use crate::types::{Condition, FileData, FoundFile};

#[derive(Default, Debug)]
pub struct FsReport {
    /// Unallocated byte ranges (absolute, sorted, merged) — what a
    /// free-space-only deep scan needs to read.
    pub free: Option<Vec<(u64, u64)>>,
    pub found: usize,
    pub cluster: u64,
}

/// Call `f(start_bit, len)` for every run of zero bits in `bm[..nbits]`.
pub(crate) fn free_runs(bm: &[u8], nbits: u64, mut f: impl FnMut(u64, u64)) {
    let mut run_start: Option<u64> = None;
    let mut i = 0u64;
    while i < nbits {
        let byte_i = (i / 8) as usize;
        // Fast path: whole 64-bit words that are all used / all free.
        if i.is_multiple_of(64) && i + 64 <= nbits && byte_i + 8 <= bm.len() {
            let w = u64::from_le_bytes(bm[byte_i..byte_i + 8].try_into().unwrap());
            if w == u64::MAX {
                if let Some(s) = run_start.take() {
                    f(s, i - s);
                }
                i += 64;
                continue;
            }
            if w == 0 {
                run_start.get_or_insert(i);
                i += 64;
                continue;
            }
        }
        let used = bm.get(byte_i).is_none_or(|b| b >> (i % 8) & 1 == 1);
        if used {
            if let Some(s) = run_start.take() {
                f(s, i - s);
            }
        } else {
            run_start.get_or_insert(i);
        }
        i += 1;
    }
    if let Some(s) = run_start {
        f(s, nbits - s);
    }
}

/// Number of set bits in `[start, start+len)`; bits past the bitmap count as set.
pub(crate) fn count_set(bm: &[u8], start: u64, len: u64) -> u64 {
    let mut n = 0;
    let mut i = start;
    let end = start + len;
    while i < end {
        let byte_i = (i / 8) as usize;
        if i.is_multiple_of(8) && i + 8 <= end {
            n += bm.get(byte_i).map_or(8, |b| b.count_ones() as u64);
            i += 8;
        } else {
            n += bm.get(byte_i).map_or(1, |b| (b >> (i % 8) & 1) as u64);
            i += 1;
        }
    }
    n
}

pub(crate) fn condition_from(reused: u64, total: u64) -> Condition {
    if total == 0 || reused == 0 {
        Condition::Good
    } else if reused >= total {
        Condition::Overwritten
    } else {
        Condition::Partial
    }
}

/// When two deleted files claim the same clusters, the one created later
/// owns the data and the earlier one has been (partly) overwritten — the
/// allocation bitmap can't tell, since both are free now. `keys` are
/// creation timestamps (any monotonic unit) parallel to `files`.
pub(crate) fn mark_overlaps(files: &mut [FoundFile], keys: &[u64]) {
    let mut iv: Vec<(u64, u64, usize)> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if let FileData::Extents(e) = &f.data {
            iv.extend(
                e.iter()
                    .filter(|x| !x.sparse && x.len > 0)
                    .map(|x| (x.offset, x.offset + x.len, i)),
            );
        }
    }
    iv.sort_unstable();
    let mut lost = vec![0u64; files.len()];
    let mut active: Vec<(u64, usize)> = Vec::new();
    for &(s, e, i) in &iv {
        active.retain(|&(ae, _)| ae > s);
        for &(ae, j) in &active {
            if j == i {
                continue;
            }
            let o = ae.min(e) - s;
            match keys[i].cmp(&keys[j]) {
                std::cmp::Ordering::Greater => lost[j] += o,
                std::cmp::Ordering::Less => lost[i] += o,
                std::cmp::Ordering::Equal => {}
            }
        }
        active.push((e, i));
    }
    for (f, l) in files.iter_mut().zip(lost) {
        if l == 0 {
            continue;
        }
        let worse = if l >= f.size {
            Condition::Overwritten
        } else {
            Condition::Partial
        };
        f.condition = f.condition.max(worse);
    }
}

/// Finish a filesystem scan: fix up conditions, then stream the results.
pub(crate) fn emit(ctx: &ScanCtx, mut files: Vec<FoundFile>, keys: &[u64]) -> usize {
    mark_overlaps(&mut files, keys);
    let n = files.len();
    let mut batch = Batch::new(ctx);
    for f in files {
        batch.push(f);
    }
    n
}

/// Merge touching/overlapping ranges in place.
pub(crate) fn merge_ranges(v: &mut Vec<(u64, u64)>) {
    v.sort_unstable_by_key(|r| r.0);
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(v.len());
    for &(s, l) in v.iter() {
        if let Some(last) = out.last_mut() {
            if s <= last.0 + last.1 {
                last.1 = last.1.max(s + l - last.0);
                continue;
            }
        }
        out.push((s, l));
    }
    *v = out;
}

/// Make a name safe for Windows/Linux/macOS file systems.
pub fn sanitize_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        s.insert(0, '_');
    }
    if s.is_empty() {
        s.push('_');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs() {
        let mut bm = vec![0xFFu8; 32];
        bm[3] = 0b1110_0001; // bits 25..=28 free
        bm[10..20].fill(0); // bits 80..160 free
        let mut got = Vec::new();
        free_runs(&bm, 256, |s, l| got.push((s, l)));
        assert_eq!(got, vec![(25, 4), (80, 80)]);
        assert_eq!(count_set(&bm, 24, 8), 4);
        assert_eq!(count_set(&bm, 80, 80), 0);
    }

    #[test]
    fn names() {
        assert_eq!(sanitize_name("a:b?.txt"), "a_b_.txt");
        assert_eq!(sanitize_name("con.txt"), "_con.txt");
        assert_eq!(sanitize_name("x. "), "x");
    }
}

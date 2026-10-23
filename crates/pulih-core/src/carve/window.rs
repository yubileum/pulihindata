//! Byte access for format resolvers: serves from the scan chunk already in
//! memory and only touches the device for data past the chunk.

use crate::device::{read_full, AlignedBuf, ReadAt};

const SEQ_FILL: usize = 1 << 20;
const SPARSE_FILL: usize = 64 << 10;
const ALIGN: u64 = 4096;

pub struct Window<'a> {
    src: &'a dyn ReadAt,
    /// Absolute offset of the candidate file's first byte.
    pub start: u64,
    /// Bytes from `start` to the end of the device.
    pub avail: u64,
    mem: &'a [u8],
    cache: AlignedBuf,
    cache_abs: u64,
    cache_len: usize,
}

impl<'a> Window<'a> {
    pub fn new(src: &'a dyn ReadAt, start: u64, mem: &'a [u8], cache: AlignedBuf) -> Self {
        let avail = src.size().saturating_sub(start);
        let mem = &mem[..mem.len().min(avail as usize)];
        Window {
            src,
            start,
            avail,
            mem,
            cache,
            cache_abs: 0,
            cache_len: 0,
        }
    }

    pub fn into_cache(self) -> AlignedBuf {
        self.cache
    }

    fn fill(&mut self, rel: u64, n: usize) {
        let abs = self.start + rel;
        let cache_end = self.cache_abs + self.cache_len as u64;
        // Sequential access pattern → big read-ahead; jumps (box walking) →
        // small reads so sparse structure walks stay cheap.
        let sequential = abs >= cache_end && abs - cache_end < 256 << 10 && self.cache_len > 0;
        let sequential = sequential || (rel as usize) < self.mem.len() + (256 << 10);
        let want = n.max(if sequential { SEQ_FILL } else { SPARSE_FILL });
        let a = abs / ALIGN * ALIGN;
        let mut len = ((abs - a) as usize + want).div_ceil(ALIGN as usize) * ALIGN as usize;
        len = len.min(self.cache.len());
        let dev_end = self.start + self.avail;
        len = len.min((dev_end - a) as usize);
        let got = read_full(self.src, a, &mut self.cache[..len]).unwrap_or(0);
        self.cache_abs = a;
        self.cache_len = got;
    }

    /// Up to `n` bytes at `rel` (fewer only at end of device / read error).
    pub fn get(&mut self, rel: u64, n: usize) -> &[u8] {
        if rel >= self.avail {
            return &[];
        }
        let n = n.min((self.avail - rel).min(usize::MAX as u64) as usize);
        let end = rel + n as u64;
        if end <= self.mem.len() as u64 {
            return &self.mem[rel as usize..end as usize];
        }
        let abs = self.start + rel;
        let hit = abs >= self.cache_abs && abs + n as u64 <= self.cache_abs + self.cache_len as u64;
        if !hit {
            if n + ALIGN as usize > self.cache.len() {
                // Larger than the cache: grow it once.
                self.cache = AlignedBuf::new(n + 2 * ALIGN as usize);
            }
            self.fill(rel, n);
        }
        let s = (abs - self.cache_abs) as usize;
        let e = (s + n).min(self.cache_len);
        if s >= e {
            return &[];
        }
        &self.cache[s..e]
    }

    /// A convenient contiguous slice starting at `rel` for forward scans
    /// (at least 4 KB unless the device ends).
    pub fn chunk(&mut self, rel: u64) -> &[u8] {
        if rel >= self.avail {
            return &[];
        }
        if rel + 4096 <= self.mem.len() as u64
            || (self.mem.len() as u64 == self.avail && rel < self.avail)
        {
            return &self.mem[rel as usize..];
        }
        let abs = self.start + rel;
        let cache_end = self.cache_abs + self.cache_len as u64;
        if abs >= self.cache_abs && abs + 4096 <= cache_end {
            let s = (abs - self.cache_abs) as usize;
            return &self.cache[s..self.cache_len];
        }
        self.fill(rel, SEQ_FILL);
        let s = (abs.saturating_sub(self.cache_abs)) as usize;
        if s >= self.cache_len {
            return &[];
        }
        &self.cache[s..self.cache_len]
    }
}

/// Watches the bytes a forward scan passes over and reports where the file
/// most likely ended: at a sector that starts another known file type, or at
/// a long run of zero sectors (wiped / TRIMmed space).
pub struct Guard {
    next: u64,
    zero_run: u64,
    zero_stop: u64,
    exclude: u64,
}

impl Guard {
    /// `start` = absolute file start; `exclude` = signature-index bitmask
    /// of types that may legitimately appear inside this file.
    pub fn new(start: u64, zero_stop: u64, exclude: u64) -> Self {
        Guard {
            next: (start / 512 + 1) * 512,
            zero_run: 0,
            zero_stop,
            exclude,
        }
    }

    /// Feed bytes `data` located at absolute offset `abs`. Returns the
    /// absolute offset where scanning should stop, if inside this range.
    pub fn feed(&mut self, stop: &super::formats::Stopper, abs: u64, data: &[u8]) -> Option<u64> {
        let end = abs + data.len() as u64;
        let mut p = self.next.max(abs.div_ceil(512) * 512);
        while p + 512 <= end {
            let s = &data[(p - abs) as usize..(p - abs) as usize + 512];
            if is_zero(s) {
                self.zero_run += 512;
                if self.zero_run >= self.zero_stop {
                    let at = p + 512 - self.zero_run;
                    self.next = p + 512;
                    return Some(at);
                }
            } else {
                self.zero_run = 0;
                if stop.is_header(s, self.exclude) {
                    self.next = p + 512;
                    return Some(p);
                }
            }
            p += 512;
        }
        self.next = p;
        None
    }
}

#[inline]
pub fn is_zero(s: &[u8]) -> bool {
    let (pre, mid, post) = unsafe { s.align_to::<u128>() };
    pre.iter().all(|&b| b == 0) && mid.iter().all(|&w| w == 0) && post.iter().all(|&b| b == 0)
}

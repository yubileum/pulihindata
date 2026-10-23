//! File signatures and structure-aware size resolvers.
//!
//! A resolver gets a [`Ctx`] positioned at a candidate header, validates it
//! and walks the format's own structure (chunks, boxes, segments, central
//! directories...) to find the exact end of the file. Containers are walked
//! by jumping over their payload, so a 4 GB video costs a handful of small
//! reads, not 4 GB of I/O.

use memchr::{memchr, memmem};

use super::window::{Guard, Window};
use crate::bytes::{crc32, utf16le, Bytes};
use crate::types::Category;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Structure walked to its terminator.
    Exact,
    /// Structure broken / truncated before its terminator.
    Partial,
    /// Format has no reliable end marker; size is a best guess.
    Estimated,
}

#[derive(Clone, Copy, Debug)]
pub struct Carve {
    pub size: u64,
    pub ext: &'static str,
    pub status: Status,
}

fn ok(size: u64, ext: &'static str) -> Option<Carve> {
    Some(Carve {
        size,
        ext,
        status: Status::Exact,
    })
}
fn partial(size: u64, ext: &'static str) -> Option<Carve> {
    Some(Carve {
        size,
        ext,
        status: Status::Partial,
    })
}
fn est(size: u64, ext: &'static str) -> Option<Carve> {
    Some(Carve {
        size,
        ext,
        status: Status::Estimated,
    })
}

pub enum Find {
    Hit(u64),
    Stop(u64),
    Limit(u64),
}

pub struct Ctx<'a, 'b> {
    pub w: Window<'a>,
    pub stop: &'b Stopper,
    /// Maximum plausible size for this format (bounded by device end).
    pub limit: u64,
}

impl Ctx<'_, '_> {
    #[inline]
    pub fn b(&mut self, rel: u64, n: usize) -> &[u8] {
        self.w.get(rel, n)
    }
    #[inline]
    pub fn arr<const N: usize>(&mut self, rel: u64) -> Option<[u8; N]> {
        self.w.get(rel, N).try_into().ok()
    }
    #[inline]
    pub fn u8(&mut self, rel: u64) -> Option<u8> {
        self.arr::<1>(rel).map(|a| a[0])
    }
    pub fn u16le(&mut self, rel: u64) -> Option<u16> {
        self.arr(rel).map(u16::from_le_bytes)
    }
    pub fn u16be(&mut self, rel: u64) -> Option<u16> {
        self.arr(rel).map(u16::from_be_bytes)
    }
    pub fn u32le(&mut self, rel: u64) -> Option<u32> {
        self.arr(rel).map(u32::from_le_bytes)
    }
    pub fn u32be(&mut self, rel: u64) -> Option<u32> {
        self.arr(rel).map(u32::from_be_bytes)
    }
    pub fn u64le(&mut self, rel: u64) -> Option<u64> {
        self.arr(rel).map(u64::from_le_bytes)
    }
    pub fn u64be(&mut self, rel: u64) -> Option<u64> {
        self.arr(rel).map(u64::from_be_bytes)
    }
    pub fn starts(&mut self, rel: u64, pat: &[u8]) -> bool {
        self.w.get(rel, pat.len()) == pat
    }

    /// Forward search for `needle` from `from` (relative), bounded by `max`
    /// and optionally cut short by a [`Guard`].
    pub fn find(
        &mut self,
        from: u64,
        needle: &[u8],
        mut guard: Option<&mut Guard>,
        max: u64,
    ) -> Find {
        let finder = memmem::Finder::new(needle);
        let max = max.min(self.limit);
        let start = self.w.start;
        let nl = needle.len();
        let mut pos = from;
        loop {
            if pos >= max {
                return Find::Limit(max);
            }
            let data = self.w.chunk(pos);
            if data.len() < nl {
                return Find::Limit(pos + data.len() as u64);
            }
            let n = data.len().min((max - pos).min(usize::MAX as u64) as usize);
            let mut scan = n;
            let mut stopped = None;
            if let Some(g) = guard.as_deref_mut() {
                if let Some(s) = g.feed(self.stop, start + pos, &data[..n]) {
                    scan = (s - start - pos) as usize;
                    stopped = Some(s - start);
                }
            }
            let hay_end = (scan + nl - 1).min(data.len());
            if let Some(i) = finder.find(&data[..hay_end]) {
                if i < scan {
                    return Find::Hit(pos + i as u64);
                }
            }
            if let Some(s) = stopped {
                return Find::Stop(s);
            }
            pos += scan.min(hay_end + 1 - nl).max(1) as u64;
        }
    }
}

pub struct Sig {
    pub name: &'static str,
    pub cats: u8,
    pub magic: &'static [u8],
    pub off: usize,
    /// Distinctive enough to mark "another file starts here".
    pub strong: bool,
    /// Too weak for byte-granular deep scans.
    pub aligned_only: bool,
    pub max: u64,
    pub quick: fn(&[u8]) -> bool,
    pub resolve: fn(&mut Ctx) -> Option<Carve>,
}

const IMG: u8 = 1 << Category::Image as u8;
const VID: u8 = 1 << Category::Video as u8;
const AUD: u8 = 1 << Category::Audio as u8;
const DOC: u8 = 1 << Category::Document as u8;
const ARC: u8 = 1 << Category::Archive as u8;
const OTH: u8 = 1 << Category::Other as u8;
const MB: u64 = 1 << 20;
const GB: u64 = 1 << 30;

fn yes(_: &[u8]) -> bool {
    true
}

macro_rules! sig {
    ($name:expr, $cats:expr, $magic:expr, $off:expr, $strong:expr, $aligned:expr, $max:expr, $quick:expr, $res:expr) => {
        Sig {
            name: $name,
            cats: $cats,
            magic: $magic,
            off: $off,
            strong: $strong,
            aligned_only: $aligned,
            max: $max,
            quick: $quick,
            resolve: $res,
        }
    };
}

pub const JPEG: usize = 0;

/// Ordered by priority: when two signatures match one position the first
/// one that resolves wins.
pub static SIGS: &[Sig] = &[
    sig!(
        "JPEG",
        IMG,
        b"\xFF\xD8\xFF",
        0,
        true,
        false,
        128 * MB,
        q_jpeg,
        jpeg
    ),
    sig!(
        "PNG",
        IMG,
        b"\x89PNG\r\n\x1a\n",
        0,
        true,
        false,
        512 * MB,
        q_png,
        png
    ),
    sig!("GIF", IMG, b"GIF8", 0, true, false, 128 * MB, q_gif, gif),
    sig!("BMP", IMG, b"BM", 0, false, false, GB, yes, bmp),
    sig!(
        "TIFF / RAW",
        IMG,
        b"II*\0",
        0,
        true,
        false,
        512 * MB,
        q_tiff,
        tiff
    ),
    sig!(
        "TIFF / RAW",
        IMG,
        b"MM\0*",
        0,
        true,
        false,
        512 * MB,
        q_tiff,
        tiff
    ),
    sig!("ORF", IMG, b"IIR", 0, false, false, 512 * MB, q_tiff, tiff),
    sig!("ORF", IMG, b"MMOR", 0, false, false, 512 * MB, q_tiff, tiff),
    sig!(
        "RW2",
        IMG,
        b"IIU\0",
        0,
        false,
        false,
        512 * MB,
        q_tiff,
        tiff
    ),
    sig!(
        "RAF",
        IMG,
        b"FUJIFILMCCD-RAW ",
        0,
        true,
        false,
        512 * MB,
        yes,
        raf
    ),
    sig!("PSD", IMG, b"8BPS", 0, true, false, 4 * GB, q_psd, psd),
    sig!(
        "RIFF (AVI/WAV/WEBP)",
        IMG | VID | AUD,
        b"RIFF",
        0,
        true,
        false,
        64 * GB,
        q_riff,
        riff
    ),
    sig!("AIFF", AUD, b"FORM", 0, true, false, 4 * GB, q_aiff, aiff),
    sig!(
        "MP4 / MOV / HEIC",
        IMG | VID | AUD,
        b"ftyp",
        4,
        true,
        false,
        256 * GB,
        q_iso,
        isobmff
    ),
    sig!(
        "MKV / WEBM",
        VID,
        b"\x1A\x45\xDF\xA3",
        0,
        true,
        false,
        256 * GB,
        yes,
        mkv
    ),
    sig!("FLV", VID, b"FLV\x01", 0, true, false, 16 * GB, q_flv, flv),
    sig!(
        "WMV / WMA",
        VID | AUD,
        b"\x30\x26\xB2\x75\x8E\x66\xCF\x11\xA6\xD9\x00\xAA\x00\x62\xCE\x6C",
        0,
        true,
        false,
        64 * GB,
        yes,
        asf
    ),
    sig!(
        "MPEG-PS",
        VID,
        b"\x00\x00\x01\xBA",
        0,
        true,
        false,
        32 * GB,
        q_mpg,
        mpeg_ps
    ),
    sig!("MPEG-TS", VID, b"\x47", 0, false, true, 64 * GB, yes, ts),
    sig!(
        "AVCHD MTS",
        VID,
        b"\x47",
        4,
        false,
        true,
        64 * GB,
        yes,
        m2ts
    ),
    sig!(
        "MP3 (ID3)",
        AUD,
        b"ID3",
        0,
        true,
        false,
        512 * MB,
        q_id3,
        id3
    ),
    sig!("MP3", AUD, b"\xFF", 0, false, true, 512 * MB, yes, mp3),
    sig!("FLAC", AUD, b"fLaC", 0, true, false, GB, yes, flac),
    sig!(
        "OGG / OPUS",
        AUD | VID,
        b"OggS",
        0,
        true,
        false,
        8 * GB,
        q_ogg,
        ogg
    ),
    sig!("MIDI", AUD, b"MThd", 0, true, false, 64 * MB, yes, midi),
    sig!("PDF", DOC, b"%PDF-", 0, true, false, 4 * GB, q_pdf, pdf),
    sig!(
        "ZIP / Office / APK",
        ARC | DOC,
        b"PK\x03\x04",
        0,
        true,
        false,
        64 * GB,
        q_zip,
        zip
    ),
    sig!(
        "DOC / XLS / PPT",
        DOC,
        b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1",
        0,
        true,
        false,
        4 * GB,
        yes,
        ole
    ),
    sig!("RTF", DOC, b"{\\rtf", 0, true, false, 512 * MB, yes, rtf),
    sig!(
        "RAR",
        ARC,
        b"Rar!\x1a\x07",
        0,
        true,
        false,
        64 * GB,
        yes,
        rar
    ),
    sig!(
        "7Z",
        ARC,
        b"7z\xBC\xAF\x27\x1C",
        0,
        true,
        false,
        64 * GB,
        yes,
        sevenz
    ),
    sig!(
        "SQLite",
        OTH,
        b"SQLite format 3\0",
        0,
        true,
        false,
        16 * GB,
        yes,
        sqlite
    ),
    sig!("EXE / DLL", OTH, b"MZ", 0, false, false, 4 * GB, yes, pe),
    sig!("ELF", OTH, b"\x7fELF", 0, true, false, 4 * GB, yes, elf),
];

/// Fast "does a known file start in this sector?" test used to cut short
/// forward scans of broken files.
pub struct Stopper {
    t0: Vec<Vec<u8>>,
    t4: Vec<Vec<u8>>,
}

impl Default for Stopper {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopper {
    pub fn new() -> Self {
        let mut t0 = vec![Vec::new(); 256];
        let mut t4 = vec![Vec::new(); 256];
        for (i, s) in SIGS.iter().enumerate() {
            if !s.strong {
                continue;
            }
            match s.off {
                0 => t0[s.magic[0] as usize].push(i as u8),
                4 => t4[s.magic[0] as usize].push(i as u8),
                _ => {}
            }
        }
        Stopper { t0, t4 }
    }

    #[inline]
    pub fn is_header(&self, s: &[u8], exclude: u64) -> bool {
        let check = |list: &Vec<u8>, off: usize| {
            list.iter().any(|&i| {
                let sig = &SIGS[i as usize];
                exclude >> i & 1 == 0
                    && s.get(off..off + sig.magic.len()) == Some(sig.magic)
                    && (sig.quick)(s)
            })
        };
        check(&self.t0[s[0] as usize], 0) || (s.len() > 4 && check(&self.t4[s[4] as usize], 4))
    }
}

#[inline]
fn be16(b: &[u8], o: usize) -> u16 {
    b.u16be(o).unwrap_or(0)
}
#[inline]
fn be32(b: &[u8], o: usize) -> u32 {
    b.u32be(o).unwrap_or(0)
}
#[inline]
fn le16(b: &[u8], o: usize) -> u16 {
    b.u16le(o).unwrap_or(0)
}
#[inline]
fn le32(b: &[u8], o: usize) -> u32 {
    b.u32le(o).unwrap_or(0)
}
#[inline]
fn rd16(b: &[u8], o: usize, le: bool) -> u16 {
    if le {
        le16(b, o)
    } else {
        be16(b, o)
    }
}
#[inline]
fn rd32(b: &[u8], o: usize, le: bool) -> u32 {
    if le {
        le32(b, o)
    } else {
        be32(b, o)
    }
}
#[inline]
fn rd64(b: &[u8], o: usize, le: bool) -> u64 {
    if le {
        b.u64le(o).unwrap_or(0)
    } else {
        b.u64be(o).unwrap_or(0)
    }
}

// --------------------------------------------------------------------- JPEG

fn q_jpeg(b: &[u8]) -> bool {
    b.len() >= 6
        && matches!(b[3], 0xE0..=0xEF | 0xDB | 0xC0..=0xC2 | 0xC4 | 0xFE | 0xDD)
        && be16(b, 4) >= 2
}

fn jpeg_partial(end: u64, sos: u64) -> Option<Carve> {
    (end > sos + 1024).then_some(Carve {
        size: end,
        ext: "jpg",
        status: Status::Partial,
    })
}

fn jpeg(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<6>(0)?;
    if !q_jpeg(&h) {
        return None;
    }
    // Header segments up to the first Start-Of-Scan.
    let mut pos = 2u64;
    let mut sof = false;
    let mut segs = 0;
    loop {
        if pos + 4 > c.limit {
            return None;
        }
        let s = c.arr::<4>(pos)?;
        if s[0] != 0xFF {
            return None;
        }
        let m = s[1];
        if m == 0xFF {
            pos += 1;
            continue;
        }
        if matches!(m, 0x00 | 0x01 | 0xD0..=0xD9) {
            return None;
        }
        let len = be16(&s, 2) as u64;
        if len < 2 {
            return None;
        }
        if matches!(m, 0xC0..=0xCF) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            sof = true;
        }
        pos += 2 + len;
        if m == 0xDA {
            break;
        }
        segs += 1;
        if segs > 1000 {
            return None;
        }
    }
    if !sof {
        return None;
    }
    // Entropy-coded data: 0xFF is always followed by 0x00 (stuffing), a
    // restart marker, or a real marker. Anything else means the data is no
    // longer this JPEG.
    let sos = pos;
    let start = c.w.start;
    let mut g = Guard::new(start, 64 << 10, 0);
    // Entropy data carries an 0xFF every few hundred bytes; a long stretch
    // without one (e.g. text left in free space) is no longer this image.
    let mut last_ff = pos;
    'outer: loop {
        if pos >= c.limit {
            return jpeg_partial(c.limit, sos);
        }
        let lim = (c.limit - pos).min(usize::MAX as u64) as usize;
        let data = c.w.chunk(pos);
        if data.len() < 2 {
            return jpeg_partial(pos + data.len() as u64, sos);
        }
        let mut n = data.len().min(lim);
        let stop = g.feed(c.stop, start + pos, &data[..n]);
        if let Some(s) = stop {
            n = (s - start - pos) as usize;
        }
        let mut j = 0;
        while j < n {
            let Some(i) = memchr(0xFF, &data[j..n]) else {
                break;
            };
            let k = j + i;
            last_ff = pos + k as u64;
            if k + 3 >= data.len() {
                if k == 0 {
                    return jpeg_partial(pos, sos);
                }
                pos += k as u64;
                continue 'outer;
            }
            match data[k + 1] {
                0x00 | 0xD0..=0xD7 => j = k + 2,
                0xFF => j = k + 1,
                0xD9 => return ok(pos + k as u64 + 2, "jpg"),
                0xC0..=0xCF | 0xDA..=0xDF | 0xE0..=0xEF | 0xFE => {
                    let len = be16(data, k + 2) as u64;
                    if len < 2 {
                        return jpeg_partial(pos + k as u64, sos);
                    }
                    pos += k as u64 + 2 + len;
                    continue 'outer;
                }
                _ => return jpeg_partial(pos + k as u64, sos),
            }
        }
        if let Some(s) = stop {
            return jpeg_partial(s - start, sos);
        }
        if pos + n as u64 - last_ff > MB {
            return jpeg_partial(last_ff, sos);
        }
        pos += n as u64;
    }
}

// ---------------------------------------------------------------------- PNG

fn q_png(b: &[u8]) -> bool {
    b.len() >= 16 && b[8..12] == [0, 0, 0, 13] && &b[12..16] == b"IHDR"
}

fn png(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<24>(0)?;
    if !q_png(&h) {
        return None;
    }
    let (w, ht) = (be32(&h, 16), be32(&h, 20));
    if w == 0 || ht == 0 || w > 1 << 20 || ht > 1 << 20 {
        return None;
    }
    let mut pos = 8u64;
    let mut chunks = 0;
    loop {
        if pos >= c.limit {
            return partial(pos, "png");
        }
        let Some(ch) = c.arr::<8>(pos) else {
            return partial(pos, "png");
        };
        let len = be32(&ch, 0) as u64;
        let ty = &ch[4..8];
        if !ty.iter().all(u8::is_ascii_alphabetic) || len > 0x7FFF_FFFF {
            return if chunks > 1 {
                partial(pos, "png")
            } else {
                None
            };
        }
        pos += 12 + len;
        chunks += 1;
        if ty == b"IEND" {
            return ok(pos, "png");
        }
    }
}

// ---------------------------------------------------------------------- GIF

fn q_gif(b: &[u8]) -> bool {
    b.len() >= 6 && (b[4] == b'7' || b[4] == b'9') && b[5] == b'a'
}

fn sub_blocks(c: &mut Ctx, mut pos: u64) -> Option<u64> {
    loop {
        let n = c.u8(pos)? as u64;
        pos += 1 + n;
        if n == 0 || pos > c.limit {
            return Some(pos);
        }
    }
}

fn gif(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<13>(0)?;
    if !q_gif(&h) {
        return None;
    }
    let mut pos = 13u64;
    if h[10] & 0x80 != 0 {
        pos += 3 << ((h[10] & 7) + 1);
    }
    let mut frames = 0;
    loop {
        if pos >= c.limit {
            return partial(pos, "gif");
        }
        let Some(t) = c.u8(pos) else {
            return if frames > 0 {
                partial(pos, "gif")
            } else {
                None
            };
        };
        match t {
            0x2C => {
                let d = c.arr::<10>(pos)?;
                pos += 10;
                if d[9] & 0x80 != 0 {
                    pos += 3 << ((d[9] & 7) + 1);
                }
                pos = sub_blocks(c, pos + 1)?;
                frames += 1;
            }
            0x21 => pos = sub_blocks(c, pos + 2)?,
            0x3B => return ok(pos + 1, "gif"),
            _ => {
                return if frames > 0 {
                    partial(pos, "gif")
                } else {
                    None
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- BMP

fn bmp(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<54>(0)?;
    let mut size = le32(&h, 2) as u64;
    let off = le32(&h, 10) as u64;
    let dib = le32(&h, 14) as u64;
    if le32(&h, 6) != 0 || !matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124) || off < 14 + dib {
        return None;
    }
    let (w, ht, planes, bpp) = if dib == 12 {
        (
            le16(&h, 18) as i64,
            le16(&h, 20) as i64,
            le16(&h, 22),
            le16(&h, 24),
        )
    } else {
        (
            le32(&h, 18) as i32 as i64,
            le32(&h, 22) as i32 as i64,
            le16(&h, 26),
            le16(&h, 28),
        )
    };
    if planes != 1
        || !matches!(bpp, 1 | 4 | 8 | 16 | 24 | 32)
        || w <= 0
        || w > 65535
        || ht == 0
        || ht.abs() > 65535
    {
        return None;
    }
    let comp = if dib >= 40 { le32(&h, 30) } else { 0 };
    if comp > 6 {
        return None;
    }
    if comp == 0 {
        let row = (w as u64 * bpp as u64).div_ceil(32) * 4;
        let need = off + row * ht.unsigned_abs();
        if size == 0 {
            size = need;
        }
        if size < need || size > need + 4096 {
            return None;
        }
    }
    if size <= off {
        return None;
    }
    ok(size, "bmp")
}

// ------------------------------------------------------------- TIFF / RAW

fn q_tiff(b: &[u8]) -> bool {
    if b.len() < 8 {
        return false;
    }
    let off = rd32(b, 4, b[0] == b'I');
    (8..1 << 28).contains(&off)
}

fn tiff_val(c: &mut Ctx, ty: u16, pos: u64, i: u64, le: bool) -> Option<u64> {
    Some(match ty {
        3 => {
            let b = c.arr::<2>(pos + i * 2)?;
            rd16(&b, 0, le) as u64
        }
        4 | 13 => {
            let b = c.arr::<4>(pos + i * 4)?;
            rd32(&b, 0, le) as u64
        }
        16 => {
            let b = c.arr::<8>(pos + i * 8)?;
            rd64(&b, 0, le)
        }
        _ => return None,
    })
}

fn tiff(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<16>(0)?;
    let le = h[0] == b'I';
    let mut ext = match &h[0..4] {
        b"II*\0" | b"MM\0*" => "tif",
        b"IIRO" | b"IIRS" | b"MMOR" => "orf",
        b"IIU\0" => "rw2",
        _ => return None,
    };
    if &h[8..10] == b"CR" && h[10] == 2 {
        ext = "cr2";
    }
    let ifd0 = rd32(&h, 4, le) as u64;
    if ifd0 < 8 || ifd0 >= c.limit {
        return None;
    }
    let mut queue = vec![ifd0];
    let mut seen: Vec<u64> = Vec::new();
    let mut end = 16u64;
    let mut make = [0u8; 9];
    let mut dng = false;
    while let Some(ifd) = queue.pop() {
        if seen.len() >= 64 || seen.contains(&ifd) || ifd >= c.limit {
            continue;
        }
        seen.push(ifd);
        let first = seen.len() == 1;
        let Some(nb) = c.arr::<2>(ifd) else {
            if first {
                return None;
            }
            continue;
        };
        let n = rd16(&nb, 0, le) as u64;
        if n == 0 || n > 2000 {
            if first {
                return None;
            }
            continue;
        }
        let ents = c.b(ifd + 2, (n * 12) as usize).to_vec();
        if ents.len() < (n * 12) as usize {
            if first {
                return None;
            }
            continue;
        }
        end = end.max(ifd + 2 + n * 12 + 4);
        type Arr = Option<(u16, u64, u64)>;
        let (mut so, mut sc, mut to, mut tc): (Arr, Arr, Arr, Arr) = (None, None, None, None);
        let (mut jo, mut jl) = (0u64, 0u64);
        for k in 0..n as usize {
            let e = &ents[k * 12..k * 12 + 12];
            let tag = rd16(e, 0, le);
            let ty = rd16(e, 2, le);
            let cnt = rd32(e, 4, le) as u64;
            let val = rd32(e, 8, le) as u64;
            let tsz: u64 = match ty {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 | 16..=18 => 8,
                _ => {
                    if first && k == 0 {
                        return None;
                    }
                    continue;
                }
            };
            let bytes = cnt.saturating_mul(tsz);
            if bytes > 4 && val.saturating_add(bytes) <= c.limit {
                end = end.max(val + bytes);
            }
            let dpos = if bytes > 4 {
                val
            } else {
                ifd + 2 + k as u64 * 12 + 8
            };
            match tag {
                0x0111 => so = Some((ty, cnt, dpos)),
                0x0117 => sc = Some((ty, cnt, dpos)),
                0x0144 => to = Some((ty, cnt, dpos)),
                0x0145 => tc = Some((ty, cnt, dpos)),
                0x0201 => jo = val,
                0x0202 => jl = val,
                0x014A | 0x8769 | 0x8825 | 0xA005 => {
                    for i in 0..cnt.min(32) {
                        if let Some(v) = tiff_val(c, if ty == 13 { 4 } else { ty }, dpos, i, le) {
                            queue.push(v);
                        }
                    }
                }
                0x010F => {
                    let s = c.b(dpos, cnt.min(9) as usize);
                    make[..s.len()].copy_from_slice(s);
                }
                0xC612 => dng = true,
                _ => {}
            }
        }
        if let Some(nx) = c.arr::<4>(ifd + 2 + n * 12) {
            let next = rd32(&nx, 0, le) as u64;
            if next != 0 {
                queue.push(next);
            }
        }
        for (o, l) in [(so, sc), (to, tc)] {
            if let (Some(o), Some(l)) = (o, l) {
                for i in 0..o.1.min(l.1).min(1 << 20) {
                    let (Some(off), Some(len)) =
                        (tiff_val(c, o.0, o.2, i, le), tiff_val(c, l.0, l.2, i, le))
                    else {
                        break;
                    };
                    let e = off.saturating_add(len);
                    if e <= c.limit {
                        end = end.max(e);
                    }
                }
            }
        }
        if jo > 0 && jl > 0 && jo + jl <= c.limit {
            end = end.max(jo + jl);
        }
    }
    if ext == "tif" {
        ext = if dng {
            "dng"
        } else if make.starts_with(b"NIKON") {
            "nef"
        } else if make.starts_with(b"SONY") {
            "arw"
        } else if make.starts_with(b"PENTAX") {
            "pef"
        } else if make.starts_with(b"SAMSUNG") {
            "srw"
        } else {
            "tif"
        };
    }
    ok(end, ext)
}

fn raf(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 108).to_vec();
    if h.len() < 108 {
        return None;
    }
    let mut end = 0u64;
    for (o, l) in [(84, 88), (92, 96), (100, 104)] {
        end = end.max(be32(&h, o) as u64 + be32(&h, l) as u64);
    }
    (end >= 108).then_some(())?;
    ok(end, "raf")
}

// ---------------------------------------------------------------------- PSD

fn q_psd(b: &[u8]) -> bool {
    b.len() >= 12 && matches!(be16(b, 4), 1 | 2) && b[6..12] == [0; 6]
}

fn psd(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<26>(0)?;
    if !q_psd(&h) {
        return None;
    }
    let big = be16(&h, 4) == 2;
    let ch = be16(&h, 12) as u64;
    let ht = be32(&h, 14) as u64;
    let w = be32(&h, 18) as u64;
    let depth = be16(&h, 22) as u64;
    if !(1..=56).contains(&ch)
        || ht == 0
        || w == 0
        || ht > 300_000
        || w > 300_000
        || !matches!(depth, 1 | 8 | 16 | 32)
        || be16(&h, 24) > 9
    {
        return None;
    }
    let ext = if big { "psb" } else { "psd" };
    let mut pos = 26u64;
    pos += 4 + c.u32be(pos)? as u64;
    pos += 4 + c.u32be(pos)? as u64;
    let lm = if big {
        let v = c.u64be(pos)?;
        pos += 8;
        v
    } else {
        let v = c.u32be(pos)? as u64;
        pos += 4;
        v
    };
    pos = pos.saturating_add(lm);
    let comp = c.u16be(pos)?;
    pos += 2;
    let row = if depth == 1 {
        w.div_ceil(8)
    } else {
        w * depth / 8
    };
    match comp {
        0 => pos += ch * ht * row,
        1 => {
            let n = ch * ht;
            let esz = if big { 4 } else { 2 };
            if n * esz > 64 * MB {
                return None;
            }
            let t = c.b(pos, (n * esz) as usize).to_vec();
            if t.len() < (n * esz) as usize {
                return partial(pos, ext);
            }
            let sum: u64 = if big {
                t.chunks_exact(4).map(|x| be32(x, 0) as u64).sum()
            } else {
                t.chunks_exact(2).map(|x| be16(x, 0) as u64).sum()
            };
            pos += n * esz + sum;
        }
        _ => return est(c.limit, ext),
    }
    ok(pos, ext)
}

// --------------------------------------------------------------------- RIFF

fn q_riff(b: &[u8]) -> bool {
    b.len() >= 12 && matches!(&b[8..12], b"WAVE" | b"AVI " | b"WEBP")
}

fn riff(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<16>(0)?;
    if !q_riff(&h) {
        return None;
    }
    if !h[12..16]
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b' ' || *b == b'_')
    {
        return None;
    }
    let ext = match &h[8..12] {
        b"WAVE" => "wav",
        b"AVI " => "avi",
        _ => "webp",
    };
    let size = le32(&h, 4) as u64;
    if size < 8 {
        return None;
    }
    let mut end = 8 + size;
    if ext == "avi" {
        // OpenDML: >1 GB AVIs continue in RIFF 'AVIX' chunks.
        for _ in 0..1024 {
            if c.starts(end, b"RIFF") && c.starts(end + 8, b"AVIX") {
                end += 8 + c.u32le(end + 4)? as u64;
            } else {
                break;
            }
        }
    }
    ok(end, ext)
}

fn q_aiff(b: &[u8]) -> bool {
    b.len() >= 12 && matches!(&b[8..12], b"AIFF" | b"AIFC")
}

fn aiff(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<12>(0)?;
    q_aiff(&h).then_some(())?;
    ok(8 + be32(&h, 4) as u64, "aif")
}

// ----------------------------------------------------- ISO BMFF (MP4, MOV, HEIC)

const ISO_BOXES: [&[u8; 4]; 27] = [
    b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"uuid", b"meta", b"pdin", b"moof",
    b"mfra", b"styp", b"sidx", b"ssix", b"prft", b"emsg", b"junk", b"pnot", b"udta", b"PICT",
    b"XMP_", b"beam", b"jumb", b"idat", b"iinf", b"iloc", b"meco",
];

fn printable4(b: &[u8]) -> bool {
    b.len() == 4 && b.iter().all(|&x| x.is_ascii_alphanumeric() || x == b' ')
}

fn q_iso(b: &[u8]) -> bool {
    if b.len() < 12 {
        return false;
    }
    let s = be32(b, 0);
    (12..=1024).contains(&s) && s.is_multiple_of(4) && printable4(&b[8..12])
}

fn iso_ext(major: &[u8], compat: &[[u8; 4]]) -> &'static str {
    let has = |x: &[u8; 4]| compat.contains(x);
    match major {
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs" => "heic",
        b"mif1" | b"msf1" => {
            if has(b"avif") {
                "avif"
            } else {
                "heic"
            }
        }
        b"avif" | b"avis" => "avif",
        b"qt  " => "mov",
        b"M4A " | b"M4B " | b"M4P " => "m4a",
        b"M4V " | b"M4VH" | b"M4VP" => "m4v",
        b"crx " => "cr3",
        b"F4V " => "f4v",
        b"3g2a" | b"3g2b" | b"3g2c" => "3g2",
        m if m.starts_with(b"3g") => "3gp",
        _ => "mp4",
    }
}

fn isobmff(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<12>(0)?;
    if !q_iso(&h) {
        return None;
    }
    let fsize = be32(&h, 0) as usize;
    let ft = c.b(0, fsize).to_vec();
    let compat: Vec<[u8; 4]> = ft
        .get(16..)
        .unwrap_or(&[])
        .chunks_exact(4)
        .map(|x| x.try_into().unwrap())
        .collect();
    let ext = iso_ext(&h[8..12], &compat);
    let mut pos = 0u64;
    let (mut moov, mut meta, mut mdat, mut moof) = (false, false, false, false);
    for _ in 0..1_000_000 {
        if pos >= c.limit {
            break;
        }
        let hd = c.b(pos, 16).to_vec();
        if hd.len() < 8 {
            break;
        }
        let ty: [u8; 4] = hd[4..8].try_into().unwrap();
        if !ISO_BOXES.contains(&&ty) || (pos > 0 && &ty == b"ftyp") {
            break;
        }
        let size = match be32(&hd, 0) {
            1 if hd.len() >= 16 => hd.u64be(8).unwrap_or(0),
            1 => break,
            0 => return est(c.limit, ext), // box runs to end of file
            s => s as u64,
        };
        if size < 8 {
            break;
        }
        match &ty {
            b"moov" => moov = true,
            b"meta" => meta = true,
            b"mdat" => mdat = true,
            b"moof" => moof = true,
            _ => {}
        }
        pos = pos.saturating_add(size);
    }
    if moov || meta || moof {
        ok(pos, ext)
    } else if mdat {
        partial(pos, ext)
    } else {
        None
    }
}

// --------------------------------------------------------------- Matroska

fn ebml_id(c: &mut Ctx, pos: u64) -> Option<(u32, u64)> {
    let b0 = c.u8(pos)?;
    let len = b0.leading_zeros() as usize + 1;
    if len > 4 {
        return None;
    }
    let b = c.b(pos, len);
    if b.len() < len {
        return None;
    }
    Some((b.iter().fold(0u32, |a, &x| a << 8 | x as u32), len as u64))
}

/// Returns (size, length of the size field); size `None` = "unknown".
fn ebml_size(c: &mut Ctx, pos: u64) -> Option<(Option<u64>, u64)> {
    let b0 = c.u8(pos)?;
    if b0 == 0 {
        return None;
    }
    let len = b0.leading_zeros() as usize + 1;
    let b = c.b(pos, len).to_vec();
    if b.len() < len {
        return None;
    }
    let mut v = (b0 & (0xFFu16 >> len) as u8) as u64;
    for &x in &b[1..] {
        v = v << 8 | x as u64;
    }
    let all_ones = v == (1u64 << (7 * len)) - 1;
    Some((if all_ones { None } else { Some(v) }, len as u64))
}

const MKV_SEG_CHILDREN: [u32; 10] = [
    0x114D_9B74,
    0x1549_A966,
    0x1654_AE6B,
    0x1F43_B675,
    0x1C53_BB6B,
    0x1941_A469,
    0x1043_A770,
    0x1254_C367,
    0xEC,
    0xBF,
];

fn mkv(c: &mut Ctx) -> Option<Carve> {
    let (id, il) = ebml_id(c, 0)?;
    if id != 0x1A45_DFA3 {
        return None;
    }
    let (sz, sl) = ebml_size(c, il)?;
    let sz = sz?;
    if sz > 4096 {
        return None;
    }
    let hdr = c.b(il + sl, sz as usize).to_vec();
    let ext = if memmem::find(&hdr, b"webm").is_some() {
        "webm"
    } else if memmem::find(&hdr, b"matroska").is_some() {
        "mkv"
    } else {
        return None;
    };
    let pos = il + sl + sz;
    let (sid, sil) = ebml_id(c, pos)?;
    if sid != 0x1853_8067 {
        return None;
    }
    let (ssz, ssl) = ebml_size(c, pos + sil)?;
    let data = pos + sil + ssl;
    if let Some(s) = ssz {
        return ok(data.saturating_add(s), ext);
    }
    // Live recordings leave the segment size "unknown": walk its children.
    let mut p = data;
    loop {
        if p >= c.limit {
            break;
        }
        let Some((id, il)) = ebml_id(c, p) else { break };
        if !MKV_SEG_CHILDREN.contains(&id) {
            break;
        }
        let Some((s, sl)) = ebml_size(c, p + il) else {
            break;
        };
        let Some(s) = s else { return est(c.limit, ext) };
        p = p.saturating_add(il + sl + s);
    }
    ok(p, ext)
}

// ---------------------------------------------------------------------- FLV

fn q_flv(b: &[u8]) -> bool {
    b.len() >= 9 && b[4] & 0xFA == 0 && be32(b, 5) == 9
}

fn flv(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<9>(0)?;
    if !q_flv(&h) {
        return None;
    }
    let mut pos = 9u64;
    let mut last = 0u64;
    let mut tags = 0;
    let end = loop {
        if pos >= c.limit {
            break pos;
        }
        let Some(prev) = c.u32be(pos) else { break pos };
        if prev as u64 != last {
            break pos;
        }
        let Some(t) = c.arr::<11>(pos + 4) else {
            break pos + 4;
        };
        if !matches!(t[0] & 0x1F, 8 | 9 | 18) || t[8..11] != [0, 0, 0] {
            break pos + 4;
        }
        let dsize = (t[1] as u64) << 16 | (t[2] as u64) << 8 | t[3] as u64;
        last = 11 + dsize;
        pos += 4 + last;
        tags += 1;
    };
    if tags == 0 {
        return None;
    }
    ok(end, "flv")
}

// ---------------------------------------------------------------------- ASF

const ASF_TOP: [[u8; 16]; 6] = [
    *b"\x30\x26\xB2\x75\x8E\x66\xCF\x11\xA6\xD9\x00\xAA\x00\x62\xCE\x6C",
    *b"\x36\x26\xB2\x75\x8E\x66\xCF\x11\xA6\xD9\x00\xAA\x00\x62\xCE\x6C",
    *b"\x90\x08\x00\x33\xB1\xE5\xCF\x11\x89\xF4\x00\xA0\xC9\x03\x49\xCB",
    *b"\xD3\x29\xE2\xD6\xDA\x35\xD1\x11\x90\x34\x00\xA0\xC9\x03\x49\xBE",
    *b"\xF8\x03\xB1\xFE\xAD\x12\x64\x4C\x84\x0F\x2A\x1D\x2F\x7A\xD4\x8C",
    *b"\xD0\x3F\xB7\x3C\x4A\x0C\x03\x48\x95\x3D\xED\xF7\xB6\x22\x8F\x0C",
];
const ASF_VIDEO: &[u8] = b"\xC0\xEF\x19\xBC\x4D\x5B\xCF\x11\xA8\xFD\x00\x80\x5F\x5C\x44\x2B";

fn asf(c: &mut Ctx) -> Option<Carve> {
    let hsize = c.u64le(16)?;
    if !(30..1 << 24).contains(&hsize) {
        return None;
    }
    let video = memmem::find(c.b(0, hsize as usize), ASF_VIDEO).is_some();
    let mut pos = 0u64;
    let mut objs = 0;
    for _ in 0..4096 {
        let Some(g) = c.arr::<24>(pos) else { break };
        if !ASF_TOP.iter().any(|t| g[0..16] == t[..]) {
            break;
        }
        let size = g.u64le(16).unwrap_or(0);
        if size < 24 {
            break;
        }
        pos = pos.saturating_add(size);
        objs += 1;
    }
    let ext = if video { "wmv" } else { "wma" };
    if objs >= 2 {
        ok(pos, ext)
    } else {
        partial(pos, ext)
    }
}

// ------------------------------------------------------------- MPEG PS / TS

fn q_mpg(b: &[u8]) -> bool {
    b.len() >= 14 && ((b[4] & 0xC4) == 0x44 || (b[4] & 0xF1) == 0x21)
}

fn mpeg_ps(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<14>(0)?;
    if !q_mpg(&h) {
        return None;
    }
    let (mut pos, mut packs, mut pes) = (0u64, 0u32, 0u32);
    loop {
        if pos >= c.limit {
            break;
        }
        let Some(b) = c.arr::<14>(pos) else { break };
        if b[0..3] != [0, 0, 1] {
            break;
        }
        match b[3] {
            0xBA => {
                pos += if b[4] >> 6 == 1 {
                    14 + (b[13] & 7) as u64
                } else {
                    12
                };
                packs += 1;
            }
            0xB9 => return ok(pos + 4, "mpg"),
            0xBB..=0xFF => {
                pos += 6 + be16(&b, 4) as u64;
                pes += 1;
            }
            _ => break,
        }
    }
    (packs >= 2 && pes >= 1).then_some(())?;
    ok(pos, "mpg")
}

fn ts_walk(c: &mut Ctx, pkt: u64, so: u64, ext: &'static str) -> Option<Carve> {
    for k in 0..16 {
        if c.u8(so + k * pkt)? != 0x47 {
            return None;
        }
    }
    // Streams start with a service-information table (PAT/NIT/SDT/SIT) with
    // payload-unit-start set — never in the middle of a PES packet.
    let h = c.arr::<3>(so)?;
    let pid = ((h[1] & 0x1F) as u16) << 8 | h[2] as u16;
    if !matches!(pid, 0 | 0x10 | 0x11 | 0x1F) || h[1] & 0x40 == 0 {
        return None;
    }
    let lim = c.limit.min(c.w.avail);
    let synced = |c: &mut Ctx, n: u64| (0..4).all(|i| c.u8(so + (n + i) * pkt) == Some(0x47));
    // Probe far ahead first (cheap), then finish packet by packet.
    let mut n = 16u64;
    const STRIDE: u64 = 4096;
    while (n + STRIDE + 4) * pkt <= lim && synced(c, n + STRIDE) {
        n += STRIDE;
    }
    while (n + 1) * pkt <= lim && c.u8(so + n * pkt) == Some(0x47) {
        n += 1;
    }
    ok(n * pkt, ext)
}

fn ts(c: &mut Ctx) -> Option<Carve> {
    ts_walk(c, 188, 0, "ts")
}

fn m2ts(c: &mut Ctx) -> Option<Carve> {
    ts_walk(c, 192, 4, "mts")
}

// --------------------------------------------------------------------- MP3

fn q_id3(b: &[u8]) -> bool {
    b.len() >= 10 && (2..=4).contains(&b[3]) && b[4] != 0xFF && b[6..10].iter().all(|&x| x < 0x80)
}

/// MPEG audio frame header → (frame length, stream key).
fn mpa_frame(h: [u8; 4]) -> Option<(u64, u32)> {
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let ver = ((h[1] >> 3) & 3) as usize;
    let layer = (h[1] >> 1) & 3;
    let bri = (h[2] >> 4) as usize;
    let sri = ((h[2] >> 2) & 3) as usize;
    if ver == 1 || layer == 0 || bri == 0 || bri == 15 || sri == 3 || h[3] & 3 == 2 {
        return None;
    }
    const SR: [[u64; 3]; 4] = [
        [11025, 12000, 8000],
        [0; 3],
        [22050, 24000, 16000],
        [44100, 48000, 32000],
    ];
    let sr = SR[ver][sri];
    let v1 = ver == 3;
    const B: [[u16; 15]; 5] = [
        [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ];
    let row = match (v1, layer) {
        (true, 3) => 0,
        (true, 2) => 1,
        (true, _) => 2,
        (false, 3) => 3,
        (false, _) => 4,
    };
    let br = B[row][bri] as u64 * 1000;
    let pad = ((h[2] >> 1) & 1) as u64;
    let len = match layer {
        3 => (12 * br / sr + pad) * 4,
        2 => 144 * br / sr + pad,
        _ if v1 => 144 * br / sr + pad,
        _ => 72 * br / sr + pad,
    };
    (len >= 24).then_some((len, (h[1] as u32 & 0xFE) << 8 | (h[2] as u32 >> 2 & 3)))
}

fn mpa_walk(c: &mut Ctx, mut pos: u64) -> (u64, u32) {
    let mut n = 0;
    let mut key = None;
    while pos < c.limit {
        let Some(h) = c.arr::<4>(pos) else { break };
        let Some((len, k)) = mpa_frame(h) else { break };
        if *key.get_or_insert(k) != k {
            break;
        }
        pos += len;
        n += 1;
    }
    (pos, n)
}

fn audio_trailer(c: &mut Ctx, mut end: u64) -> u64 {
    if c.starts(end, b"APETAGEX") {
        if let Some(sz) = c.u32le(end + 12) {
            end += 32 + sz as u64;
        }
    }
    if c.starts(end, b"TAG") {
        end += 128;
    }
    end
}

fn id3(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<10>(0)?;
    if !q_id3(&h) {
        return None;
    }
    let size = h[6..10].iter().fold(0u64, |a, &x| a << 7 | x as u64);
    let mut pos = 10 + size + if h[3] == 4 && h[5] & 0x10 != 0 { 10 } else { 0 };
    // Some taggers pad past the declared size.
    for _ in 0..16 {
        let s = c.b(pos, 4096);
        if s.is_empty() {
            break;
        }
        match s.iter().position(|&b| b != 0) {
            Some(i) => {
                pos += i as u64;
                break;
            }
            None => pos += s.len() as u64,
        }
    }
    let (end, n) = mpa_walk(c, pos);
    if n < 2 {
        return None;
    }
    ok(audio_trailer(c, end), "mp3")
}

fn mp3(c: &mut Ctx) -> Option<Carve> {
    mpa_frame(c.arr::<4>(0)?)?;
    let (end, n) = mpa_walk(c, 0);
    if n < 10 {
        return None;
    }
    ok(audio_trailer(c, end), "mp3")
}

// --------------------------------------------------------------------- FLAC

const fn crc8_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u8;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x07
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

const fn crc16_table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u16) << 8;
        let mut k = 0;
        while k < 8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x8005
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC8: [u8; 256] = crc8_table();
static CRC16: [u16; 256] = crc16_table();

/// Validate a FLAC frame header (sync, fields, CRC-8). Returns its length.
fn flac_frame_header(b: &[u8]) -> Option<usize> {
    if b.len() < 6 || b[0] != 0xFF || b[1] & 0xFE != 0xF8 {
        return None;
    }
    let (bs, sr) = (b[2] >> 4, b[2] & 0xF);
    let (ch, ss) = (b[3] >> 4, (b[3] >> 1) & 7);
    if bs == 0 || sr == 15 || ch > 10 || ss == 3 || b[3] & 1 != 0 {
        return None;
    }
    // UTF-8-style coded frame/sample number.
    let lead = b[4];
    let n = match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        0xF8..=0xFB => 5,
        0xFC..=0xFD => 6,
        0xFE => 7,
        _ => return None,
    };
    let mut len = 4 + n;
    if b.len() < len + 4 || !b[5..4 + n].iter().all(|&x| x & 0xC0 == 0x80) {
        return None;
    }
    len += match bs {
        6 => 1,
        7 => 2,
        _ => 0,
    };
    len += match sr {
        12 => 1,
        13 | 14 => 2,
        _ => 0,
    };
    let crc = b[..len].iter().fold(0u8, |c, &x| CRC8[(c ^ x) as usize]);
    (b.get(len) == Some(&crc)).then_some(len + 1)
}

fn flac(c: &mut Ctx) -> Option<Carve> {
    let mut pos = 4u64;
    let mut first = true;
    let (mut min_frame, mut max_frame) = (0u64, 0u64);
    loop {
        let bh = c.arr::<4>(pos)?;
        let ty = bh[0] & 0x7F;
        let len = (bh[1] as u64) << 16 | (bh[2] as u64) << 8 | bh[3] as u64;
        if first {
            if ty != 0 || len != 34 {
                return None;
            }
            let si = c.b(pos + 4, 34).to_vec();
            let u24 = |o: usize| {
                si.get(o..o + 3).map_or(0, |x| {
                    (x[0] as u64) << 16 | (x[1] as u64) << 8 | x[2] as u64
                })
            };
            min_frame = u24(4);
            max_frame = u24(7);
        } else if ty > 6 {
            return None;
        }
        first = false;
        pos += 4 + len;
        if bh[0] & 0x80 != 0 {
            break;
        }
        if pos > 16 * MB {
            return None;
        }
    }
    let h = c.b(pos, 16).to_vec();
    flac_frame_header(&h)?;
    let min_frame = min_frame.max(8);
    let max_frame = if max_frame == 0 { MB } else { max_frame + 64 };
    // Frames carry no length, but each ends with a CRC-16 over the whole
    // frame — so a running CRC hits zero exactly at the frame's end. Walk
    // frame to frame with that, which gives the exact file size.
    let mut frame = pos;
    let mut crc: u16 = 0;
    let mut p = pos;
    let mut first_zero: Option<u64> = None;
    let mut frames = 0u64;
    let end = 'walk: loop {
        if p >= c.limit {
            break first_zero.unwrap_or(c.limit);
        }
        let data = c.w.chunk(p).to_vec();
        if data.is_empty() {
            break first_zero.unwrap_or(p);
        }
        let n = data.len().min((c.limit - p) as usize);
        for i in 0..n {
            let at = p + i as u64;
            let len = at - frame;
            if crc == 0 && len >= min_frame {
                if data[i] == 0xFF && (i + 1 >= n || data[i + 1] & 0xFE == 0xF8) {
                    let hdr = if i + 16 <= data.len() {
                        data[i..i + 16].to_vec()
                    } else {
                        c.b(at, 16).to_vec()
                    };
                    if flac_frame_header(&hdr).is_some() {
                        frame = at;
                        frames += 1;
                        first_zero = None;
                    }
                }
                if frame != at && first_zero.is_none() {
                    first_zero = Some(at);
                }
            }
            if at - frame > max_frame {
                break 'walk first_zero.unwrap_or(frame);
            }
            crc = (crc << 8) ^ CRC16[((crc >> 8) as u8 ^ data[i]) as usize];
        }
        p += n as u64;
    };
    if frames == 0 && first_zero.is_none() {
        return None;
    }
    let end = audio_trailer(c, end);
    ok(end, "flac")
}

// ---------------------------------------------------------------------- OGG

fn q_ogg(b: &[u8]) -> bool {
    b.len() >= 27 && b[4] == 0 && b[5] & 0x02 != 0 && b[5] & !0x07 == 0
}

fn ogg(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<27>(0)?;
    if !q_ogg(&h) {
        return None;
    }
    let ns = h[26] as u64;
    let first = c.b(27 + ns, 8).to_vec();
    let ext = if first.starts_with(b"OpusHead") {
        "opus"
    } else if first.starts_with(b"\x80theora") {
        "ogv"
    } else if first.starts_with(b"\x7FFLAC") {
        "oga"
    } else if first.starts_with(b"Speex") {
        "spx"
    } else {
        "ogg"
    };
    let mut pos = 0u64;
    let mut pages = 0u64;
    while pos < c.limit {
        let Some(p) = c.arr::<27>(pos) else { break };
        if &p[0..4] != b"OggS" || p[4] != 0 {
            break;
        }
        let ns = p[26] as u64;
        let lace = c.b(pos + 27, ns as usize);
        if (lace.len() as u64) < ns {
            break;
        }
        let body: u64 = lace.iter().map(|&x| x as u64).sum();
        pos += 27 + ns + body;
        pages += 1;
    }
    (pages >= 1).then_some(())?;
    ok(pos, ext)
}

fn midi(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<14>(0)?;
    if be32(&h, 4) != 6 || be16(&h, 8) > 2 || be16(&h, 10) == 0 {
        return None;
    }
    let ntr = be16(&h, 10);
    let mut pos = 14u64;
    let mut tr = 0;
    while tr < ntr && pos < c.limit {
        let Some(ch) = c.arr::<8>(pos) else { break };
        if !ch[0..4].iter().all(u8::is_ascii_alphabetic) {
            break;
        }
        pos += 8 + be32(&ch, 4) as u64;
        if &ch[0..4] == b"MTrk" {
            tr += 1;
        }
    }
    (tr > 0).then_some(())?;
    ok(pos, "mid")
}

// ---------------------------------------------------------------------- PDF

fn q_pdf(b: &[u8]) -> bool {
    b.len() >= 8 && matches!(b[5], b'1' | b'2') && b[6] == b'.'
}

/// "12 0 obj" — the start of another (incrementally added) PDF object.
fn looks_like_obj(s: &[u8]) -> bool {
    let mut i = 0;
    let digits = |i: &mut usize| {
        let st = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > st
    };
    if !digits(&mut i) || s.get(i) != Some(&b' ') {
        return false;
    }
    i += 1;
    if !digits(&mut i) || s.get(i) != Some(&b' ') {
        return false;
    }
    s.get(i + 1..i + 4) == Some(b"obj")
}

fn pdf(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 1024).to_vec();
    if !q_pdf(&h) {
        return None;
    }
    // Linearized PDFs state their exact length up front.
    if let Some(i) = memmem::find(&h, b"/Linearized") {
        if let Some(j) = memmem::find(&h[i..], b"/L ") {
            let digits: Vec<u8> = h[i + j + 3..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .copied()
                .collect();
            if let Some(len) = std::str::from_utf8(&digits)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
            {
                if len > 1024 && len <= c.limit {
                    let tail = c.b(len - 64, 64);
                    if memmem::find(tail, b"%%EOF").is_some() {
                        return ok(len, "pdf");
                    }
                }
            }
        }
    }
    let start = c.w.start;
    // PDFs embed raw JPEG streams, so a JPEG header must not end the scan.
    let mut g = Guard::new(start, MB, 1 << JPEG);
    let mut pos = 8u64;
    let mut last_end: Option<u64> = None;
    loop {
        // First trailer within 1 GB; incremental updates are small, so a
        // later one must follow within 64 MB.
        let max = match last_end {
            None => GB,
            Some(e) => e + 64 * MB,
        };
        match c.find(pos, b"%%EOF", Some(&mut g), max) {
            Find::Hit(q) => {
                let mut end = q + 5;
                let t = c.b(end, 2).to_vec();
                if t.first() == Some(&b'\r') {
                    end += 1;
                    if t.get(1) == Some(&b'\n') {
                        end += 1;
                    }
                } else if t.first() == Some(&b'\n') {
                    end += 1;
                }
                last_end = Some(end);
                let la = c.b(end, 64).to_vec();
                let more = match la.iter().position(|b| !b" \r\n\t\x0c".contains(b)) {
                    None => false,
                    Some(k) => {
                        let s = &la[k..];
                        looks_like_obj(s)
                            || s.starts_with(b"xref")
                            || s.starts_with(b"trailer")
                            || s.starts_with(b"startxref")
                            || (s.starts_with(b"%") && !s.starts_with(b"%PDF"))
                    }
                };
                if !more {
                    return ok(end, "pdf");
                }
                pos = end;
            }
            Find::Stop(s) | Find::Limit(s) => {
                return match last_end {
                    Some(e) => ok(e, "pdf"),
                    None => partial(s, "pdf"),
                };
            }
        }
    }
}

// ---------------------------------------------------------------------- ZIP

fn q_zip(b: &[u8]) -> bool {
    b.len() >= 30
        && (le16(b, 4) & 0xFF) <= 100
        && matches!(
            le16(b, 8),
            0 | 1 | 6 | 8 | 9 | 12 | 14 | 19 | 93 | 95 | 96 | 97 | 98 | 99
        )
        && (1..=1024).contains(&le16(b, 26))
}

#[derive(Default)]
struct ZipKind {
    content_types: bool,
    word: bool,
    xl: bool,
    ppt: bool,
    visio: bool,
    android: bool,
    manifest: bool,
    mime: Option<Vec<u8>>,
}

impl ZipKind {
    fn see(&mut self, n: &[u8]) {
        self.content_types |= n == b"[Content_Types].xml";
        self.word |= n.starts_with(b"word/");
        self.xl |= n.starts_with(b"xl/");
        self.ppt |= n.starts_with(b"ppt/");
        self.visio |= n.starts_with(b"visio/");
        self.android |= n == b"AndroidManifest.xml" || n == b"classes.dex";
        self.manifest |= n == b"META-INF/MANIFEST.MF";
    }

    fn ext(&self) -> &'static str {
        if let Some(m) = &self.mime {
            return match m.as_slice() {
                b"application/vnd.oasis.opendocument.text" => "odt",
                b"application/vnd.oasis.opendocument.spreadsheet" => "ods",
                b"application/vnd.oasis.opendocument.presentation" => "odp",
                b"application/vnd.oasis.opendocument.graphics" => "odg",
                b"application/epub+zip" => "epub",
                _ => "zip",
            };
        }
        if self.content_types {
            if self.word {
                return "docx";
            }
            if self.xl {
                return "xlsx";
            }
            if self.ppt {
                return "pptx";
            }
            if self.visio {
                return "vsdx";
            }
        }
        if self.android {
            "apk"
        } else if self.manifest {
            "jar"
        } else {
            "zip"
        }
    }
}

/// Entry written with a trailing data descriptor and no size up front:
/// find where its compressed data ends.
fn zip_after_data(c: &mut Ctx, data: u64) -> Option<u64> {
    let max = data.saturating_add(256 * MB);
    let start = c.w.start;
    let mut g = Guard::new(start + data, MB, !0);
    let mut from = data;
    loop {
        match c.find(from, b"PK", Some(&mut g), max) {
            Find::Hit(q) => {
                match c.u16le(q + 2)? {
                    0x0807 => {
                        if c.u32le(q + 8)? as u64 == q - data {
                            return Some(q + 16);
                        }
                        if c.u64le(q + 8)? == q - data {
                            return Some(q + 24);
                        }
                    }
                    0x0403 | 0x0201
                        if q >= data + 12 && c.u32le(q - 8)? as u64 == q - 12 - data =>
                    {
                        return Some(q);
                    }
                    _ => {}
                }
                from = q + 1;
            }
            _ => return None,
        }
    }
}

fn zip(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<30>(0)?;
    if !q_zip(&h) {
        return None;
    }
    let mut k = ZipKind::default();
    let mut pos = 0u64;
    let mut entries = 0u64;
    while pos < c.limit {
        let Some(sig) = c.u32le(pos) else { break };
        match sig {
            0x0403_4b50 => {
                let Some(hd) = c.arr::<30>(pos) else { break };
                let flags = le16(&hd, 6);
                let method = le16(&hd, 8);
                let mut csz = le32(&hd, 18) as u64;
                let usz = le32(&hd, 22);
                let nlen = le16(&hd, 26) as u64;
                let xlen = le16(&hd, 28) as u64;
                let name = c.b(pos + 30, nlen as usize).to_vec();
                k.see(&name);
                let data = pos + 30 + nlen + xlen;
                let zip64 = csz == 0xFFFF_FFFF || usz == 0xFFFF_FFFF;
                if zip64 {
                    let extra = c.b(pos + 30 + nlen, xlen as usize).to_vec();
                    let mut p = 0;
                    while p + 4 <= extra.len() {
                        let (id, sz) = (le16(&extra, p), le16(&extra, p + 2) as usize);
                        if id == 1 {
                            let q = p + 4 + if usz == 0xFFFF_FFFF { 8 } else { 0 };
                            if csz == 0xFFFF_FFFF {
                                csz = extra.u64le(q).unwrap_or(csz);
                            }
                            break;
                        }
                        p += 4 + sz;
                    }
                }
                if entries == 0 && name == b"mimetype" && method == 0 && csz <= 128 {
                    k.mime = Some(c.b(data, csz as usize).to_vec());
                }
                if flags & 8 != 0 && csz == 0 {
                    match zip_after_data(c, data) {
                        Some(next) => pos = next,
                        None => break,
                    }
                } else {
                    pos = data.saturating_add(csz);
                    if flags & 8 != 0 {
                        let sig = c.u32le(pos) == Some(0x0807_4b50);
                        pos += if zip64 { 20 } else { 12 } + if sig { 4 } else { 0 };
                    }
                }
                entries += 1;
            }
            0x0201_4b50 => {
                let Some(hd) = c.arr::<46>(pos) else { break };
                let (n, x, cm) = (
                    le16(&hd, 28) as u64,
                    le16(&hd, 30) as u64,
                    le16(&hd, 32) as u64,
                );
                if entries == 0 {
                    let name = c.b(pos + 46, n as usize).to_vec();
                    k.see(&name);
                }
                pos += 46 + n + x + cm;
            }
            0x0606_4b50 => {
                let Some(s) = c.u64le(pos + 4) else { break };
                pos = pos.saturating_add(12 + s);
            }
            0x0706_4b50 => pos += 20,
            0x0605_4b50 => {
                let cl = c.u16le(pos + 20).unwrap_or(0) as u64;
                return ok(pos + 22 + cl, k.ext());
            }
            0x0505_4b50 => pos += 6 + c.u16le(pos + 4).unwrap_or(0) as u64,
            0x0807_4b50 => pos += 16,
            _ => break,
        }
    }
    if entries > 0 {
        partial(pos.min(c.limit), k.ext())
    } else {
        None
    }
}

// ------------------------------------------------- OLE2 (DOC/XLS/PPT/MSG)

fn ole(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 512).to_vec();
    if h.len() < 512 || le16(&h, 28) != 0xFFFE || le16(&h, 32) != 6 {
        return None;
    }
    let (ver, shift) = (le16(&h, 26), le16(&h, 30));
    if !((ver == 3 && shift == 9) || (ver == 4 && shift == 12)) {
        return None;
    }
    let ssz = 1u64 << shift;
    let nfat = le32(&h, 44) as usize;
    if nfat == 0 || nfat > 1 << 20 {
        return None;
    }
    let mut fat_secs: Vec<u32> = (0..nfat.min(109)).map(|i| le32(&h, 76 + 4 * i)).collect();
    let mut difat = le32(&h, 68);
    let mut max_used = 0u64;
    for _ in 0..le32(&h, 72).min(1 << 16) {
        if difat >= 0xFFFF_FFFA {
            break;
        }
        max_used = max_used.max(difat as u64);
        let s = c.b((difat as u64 + 1) * ssz, ssz as usize).to_vec();
        if (s.len() as u64) < ssz {
            break;
        }
        for j in 0..(ssz as usize / 4 - 1) {
            if fat_secs.len() >= nfat {
                break;
            }
            fat_secs.push(le32(&s, 4 * j));
        }
        difat = le32(&s, ssz as usize - 4);
    }
    let mut fat: Vec<u32> = Vec::with_capacity(fat_secs.len() * ssz as usize / 4);
    for &s in &fat_secs {
        if s >= 0xFFFF_FFFA {
            fat.extend(std::iter::repeat_n(0xFFFF_FFFF, ssz as usize / 4));
            continue;
        }
        max_used = max_used.max(s as u64);
        let b = c.b((s as u64 + 1) * ssz, ssz as usize).to_vec();
        if (b.len() as u64) < ssz {
            return partial((s as u64 + 1) * ssz, "doc");
        }
        for j in 0..ssz as usize / 4 {
            let v = le32(&b, 4 * j);
            if v != 0xFFFF_FFFF {
                max_used = max_used.max(fat.len() as u64);
            }
            fat.push(v);
        }
    }
    if fat.is_empty() {
        return None;
    }
    let size = (max_used + 2) * ssz;
    let mut ext = "ole";
    let mut sec = le32(&h, 48);
    for _ in 0..64 {
        if sec >= 0xFFFF_FFFA {
            break;
        }
        let b = c.b((sec as u64 + 1) * ssz, ssz as usize).to_vec();
        for e in b.chunks_exact(128) {
            let nl = le16(e, 64) as usize;
            if !(4..=64).contains(&nl) {
                continue;
            }
            let name = utf16le(&e[..nl - 2]);
            ext = match name.as_str() {
                "WordDocument" => "doc",
                "Workbook" | "Book" => "xls",
                "PowerPoint Document" => "ppt",
                "VisioDocument" => "vsd",
                n if n.starts_with("__substg1.0_") => "msg",
                _ => ext,
            };
        }
        if ext != "ole" {
            break;
        }
        sec = *fat.get(sec as usize)?;
    }
    ok(size, ext)
}

// ---------------------------------------------------------------------- RTF

fn rtf(c: &mut Ctx) -> Option<Carve> {
    let mut depth: i64 = 0;
    let mut esc = false;
    let mut pos = 0u64;
    loop {
        if pos >= c.limit {
            return partial(pos, "rtf");
        }
        let lim = (c.limit - pos) as usize;
        let data = c.w.chunk(pos);
        if data.is_empty() {
            return partial(pos, "rtf");
        }
        let n = data.len().min(lim);
        for (i, &b) in data[..n].iter().enumerate() {
            if esc {
                esc = false;
                continue;
            }
            match b {
                b'\\' => esc = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return ok(pos + i as u64 + 1, "rtf");
                    }
                }
                0..=8 | 11 | 12 | 14..=31 | 127 => {
                    let at = pos + i as u64;
                    return if at > 64 { partial(at, "rtf") } else { None };
                }
                _ => {}
            }
        }
        pos += n as u64;
    }
}

// ------------------------------------------------------------- RAR / 7z

fn vint5(c: &mut Ctx, pos: u64) -> Option<(u64, u64)> {
    let mut v = 0u64;
    for i in 0..10 {
        let b = c.u8(pos + i)?;
        v |= ((b & 0x7F) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

fn rar(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<8>(0)?;
    match (h[6], h[7]) {
        (0, _) => rar4(c),
        (1, 0) => rar5(c),
        _ => None,
    }
}

fn rar4(c: &mut Ctx) -> Option<Carve> {
    let mut pos = 7u64;
    let mut blocks = 0;
    while pos < c.limit {
        let Some(b) = c.arr::<7>(pos) else { break };
        let ty = b[2];
        let flags = le16(&b, 3);
        let hs = le16(&b, 5) as u64;
        if !(0x72..=0x7B).contains(&ty) || hs < 7 || (blocks == 0 && ty != 0x73) {
            break;
        }
        let hdr = c.b(pos, hs as usize).to_vec();
        if (hdr.len() as u64) < hs {
            break;
        }
        let mut add = 0u64;
        if flags & 0x8000 != 0 {
            add = le32(&hdr, 7) as u64;
        }
        if (ty == 0x74 || ty == 0x7A) && flags & 0x100 != 0 {
            add |= (le32(&hdr, 32) as u64) << 32;
        }
        if ty == 0x7B {
            return ok(pos + hs, "rar");
        }
        pos += hs + add;
        blocks += 1;
    }
    (blocks >= 2).then_some(())?;
    ok(pos, "rar")
}

fn rar5(c: &mut Ctx) -> Option<Carve> {
    let mut pos = 8u64;
    let mut blocks = 0;
    while pos < c.limit {
        let Some(crc) = c.u32le(pos) else { break };
        let Some((hs, l1)) = vint5(c, pos + 4) else {
            break;
        };
        if hs == 0 || hs > 2 * MB {
            break;
        }
        let hstart = pos + 4 + l1;
        let hdr = c.b(pos + 4, (l1 + hs) as usize).to_vec();
        if (hdr.len() as u64) < l1 + hs || crc32(&hdr) != crc {
            break;
        }
        let (ty, l2) = vint5(c, hstart)?;
        let (flags, l3) = vint5(c, hstart + l2)?;
        let mut p = hstart + l2 + l3;
        if flags & 1 != 0 {
            p += vint5(c, p)?.1;
        }
        let data = if flags & 2 != 0 { vint5(c, p)?.0 } else { 0 };
        if ty == 5 {
            return ok(hstart + hs, "rar");
        }
        if !(1..=4).contains(&ty) {
            break;
        }
        pos = (hstart + hs).saturating_add(data);
        blocks += 1;
    }
    (blocks >= 2).then_some(())?;
    partial(pos, "rar")
}

fn sevenz(c: &mut Ctx) -> Option<Carve> {
    let h = c.arr::<32>(0)?;
    if h[6] != 0 || crc32(&h[12..32]) != le32(&h, 8) {
        return None;
    }
    let off = h.u64le(12)?;
    let sz = h.u64le(20)?;
    if sz == 0 || sz > 1 << 32 {
        return None;
    }
    ok(32u64.checked_add(off)?.checked_add(sz)?, "7z")
}

// --------------------------------------------------------- SQLite, PE, ELF

fn sqlite(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 100).to_vec();
    if h.len() < 100 || h[21] != 64 || h[22] != 32 || h[23] != 32 {
        return None;
    }
    let ps = match be16(&h, 16) {
        1 => 65536,
        p if p.is_power_of_two() && p >= 512 => p as u64,
        _ => return None,
    };
    let n = be32(&h, 28) as u64;
    if n > 0 && be32(&h, 24) == be32(&h, 92) {
        ok(ps * n, "sqlite")
    } else {
        est(c.limit, "sqlite")
    }
}

fn pe(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 64).to_vec();
    if h.len() < 64 {
        return None;
    }
    let lfanew = le32(&h, 0x3C) as u64;
    if !(0x40..=0x1000).contains(&lfanew) {
        return None;
    }
    let p = c.b(lfanew, 264).to_vec();
    if p.len() < 264 || &p[0..4] != b"PE\0\0" {
        return None;
    }
    if !matches!(
        le16(&p, 4),
        0x14c | 0x8664 | 0x1c0 | 0x1c4 | 0xaa64 | 0x200 | 0x5032 | 0x5064
    ) {
        return None;
    }
    let nsec = le16(&p, 6) as u64;
    let optsz = le16(&p, 20) as u64;
    let chars = le16(&p, 22);
    if nsec == 0 || nsec > 96 || optsz < 96 {
        return None;
    }
    let opt = &p[24..];
    let plus = match le16(opt, 0) {
        0x10B => false,
        0x20B => true,
        _ => return None,
    };
    let mut end = le32(opt, 60) as u64;
    let subsystem = le16(opt, 68);
    let (nrva_at, dd) = if plus { (108, 112) } else { (92, 96) };
    let cert = if le32(opt, nrva_at) > 4 {
        (le32(opt, dd + 32) as u64, le32(opt, dd + 36) as u64)
    } else {
        (0, 0)
    };
    let st = c.b(lfanew + 24 + optsz, (nsec * 40) as usize).to_vec();
    if (st.len() as u64) < nsec * 40 {
        return None;
    }
    for s in st.chunks_exact(40) {
        let (rs, rp) = (le32(s, 16) as u64, le32(s, 20) as u64);
        if rs > 0 {
            end = end.max(rp + rs);
        }
    }
    if cert.0 > 0 && cert.1 > 0 {
        end = end.max(cert.0 + cert.1);
    }
    // COFF symbol + string table (MinGW / debug builds append them).
    let (symptr, nsyms) = (le32(&p, 12) as u64, le32(&p, 16) as u64);
    if symptr > 0 && nsyms > 0 && nsyms < 1 << 24 {
        let strtab = symptr + nsyms * 18;
        if let Some(n) = c.u32le(strtab) {
            if (4..1 << 28).contains(&n) {
                end = end.max(strtab + n as u64);
            }
        }
    }
    let ext = if chars & 0x2000 != 0 {
        "dll"
    } else if subsystem == 1 {
        "sys"
    } else {
        "exe"
    };
    ok(end, ext)
}

fn elf(c: &mut Ctx) -> Option<Carve> {
    let h = c.b(0, 64).to_vec();
    if h.len() < 52 || !matches!(h[4], 1 | 2) || !matches!(h[5], 1 | 2) || h[6] != 1 {
        return None;
    }
    let (b64, le) = (h[4] == 2, h[5] == 1);
    let (phoff, shoff, ehsz, phes, phn, shes, shn) = if b64 {
        if h.len() < 64 {
            return None;
        }
        (
            rd64(&h, 32, le),
            rd64(&h, 40, le),
            rd16(&h, 52, le),
            rd16(&h, 54, le),
            rd16(&h, 56, le),
            rd16(&h, 58, le),
            rd16(&h, 60, le),
        )
    } else {
        (
            rd32(&h, 28, le) as u64,
            rd32(&h, 32, le) as u64,
            rd16(&h, 40, le),
            rd16(&h, 42, le),
            rd16(&h, 44, le),
            rd16(&h, 46, le),
            rd16(&h, 48, le),
        )
    };
    let (want_eh, want_ph, want_sh) = if b64 { (64, 56, 64) } else { (52, 32, 40) };
    if ehsz != want_eh || (phn != 0 && phes != want_ph) || (shn != 0 && shes != want_sh) {
        return None;
    }
    let mut end = (ehsz as u64)
        .max(phoff + phn as u64 * phes as u64)
        .max(shoff + shn as u64 * shes as u64);
    let sh = c.b(shoff, shn as usize * shes as usize).to_vec();
    for s in sh.chunks_exact(shes.max(1) as usize) {
        let ty = rd32(s, 4, le);
        let (off, size) = if b64 {
            (rd64(s, 24, le), rd64(s, 32, le))
        } else {
            (rd32(s, 16, le) as u64, rd32(s, 20, le) as u64)
        };
        if ty != 8 && off.saturating_add(size) <= c.limit {
            end = end.max(off + size);
        }
    }
    let ph = c.b(phoff, phn as usize * phes as usize).to_vec();
    for p in ph.chunks_exact(phes.max(1) as usize) {
        let (off, size) = if b64 {
            (rd64(p, 8, le), rd64(p, 32, le))
        } else {
            (rd32(p, 4, le) as u64, rd32(p, 16, le) as u64)
        };
        if off.saturating_add(size) <= c.limit {
            end = end.max(off + size);
        }
    }
    ok(end, "elf")
}

/// Every extension a signature's resolver can report.
pub fn sig_exts(s: &Sig) -> &'static [&'static str] {
    match s.name {
        "JPEG" => &["jpg"],
        "PNG" => &["png"],
        "GIF" => &["gif"],
        "BMP" => &["bmp"],
        "TIFF / RAW" => &["tif", "cr2", "nef", "arw", "dng", "pef", "srw"],
        "ORF" => &["orf"],
        "RW2" => &["rw2"],
        "RAF" => &["raf"],
        "PSD" => &["psd", "psb"],
        "RIFF (AVI/WAV/WEBP)" => &["avi", "wav", "webp"],
        "AIFF" => &["aif"],
        "MP4 / MOV / HEIC" => &[
            "mp4", "mov", "m4a", "m4v", "3gp", "3g2", "heic", "avif", "cr3", "f4v",
        ],
        "MKV / WEBM" => &["mkv", "webm"],
        "FLV" => &["flv"],
        "WMV / WMA" => &["wmv", "wma"],
        "MPEG-PS" => &["mpg"],
        "MPEG-TS" => &["ts"],
        "AVCHD MTS" => &["mts"],
        "MP3 (ID3)" | "MP3" => &["mp3"],
        "FLAC" => &["flac"],
        "OGG / OPUS" => &["ogg", "opus", "ogv", "oga", "spx"],
        "MIDI" => &["mid"],
        "PDF" => &["pdf"],
        "ZIP / Office / APK" => &[
            "zip", "docx", "xlsx", "pptx", "vsdx", "odt", "ods", "odp", "odg", "epub", "apk", "jar",
        ],
        "DOC / XLS / PPT" => &["doc", "xls", "ppt", "vsd", "msg", "ole"],
        "RTF" => &["rtf"],
        "RAR" => &["rar"],
        "7Z" => &["7z"],
        "SQLite" => &["sqlite"],
        "EXE / DLL" => &["exe", "dll", "sys"],
        "ELF" => &["elf"],
        _ => &[],
    }
}

/// Leading magic bytes (and their offset) that a file with this extension
/// must start with — used to validate guesses made from damaged metadata.
pub fn magic_for_ext(ext: &str) -> Option<(&'static [u8], usize)> {
    Some(match ext {
        "jpg" | "jpeg" | "jpe" | "jfif" => (b"\xFF\xD8\xFF", 0),
        "png" => (b"\x89PNG", 0),
        "gif" => (b"GIF8", 0),
        "bmp" => (b"BM", 0),
        "tif" | "tiff" | "cr2" | "nef" | "arw" | "dng" | "pef" | "srw" => {
            return None; // II or MM
        }
        "psd" | "psb" => (b"8BPS", 0),
        "pdf" => (b"%PDF-", 0),
        "zip" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub" | "apk" | "jar" => {
            (b"PK\x03\x04", 0)
        }
        "doc" | "xls" | "ppt" | "msg" => (b"\xD0\xCF\x11\xE0", 0),
        "mp4" | "mov" | "m4a" | "m4v" | "3gp" | "heic" | "heif" | "avif" | "cr3" => (b"ftyp", 4),
        "mp3" => (b"ID3", 0),
        "wav" | "avi" | "webp" => (b"RIFF", 0),
        "mkv" | "webm" => (b"\x1A\x45\xDF\xA3", 0),
        "flac" => (b"fLaC", 0),
        "ogg" | "opus" | "oga" | "ogv" => (b"OggS", 0),
        "rar" => (b"Rar!", 0),
        "7z" => (b"7z\xBC\xAF", 0),
        "rtf" => (b"{\\rtf", 0),
        "exe" | "dll" | "sys" => (b"MZ", 0),
        "sqlite" | "db" => (b"SQLite format 3", 0),
        _ => return None,
    })
}

/// Signature names for the UI, deduplicated, with their category masks.
pub fn signature_names() -> Vec<(&'static str, u8)> {
    let mut v: Vec<(&'static str, u8)> = Vec::new();
    for s in SIGS {
        if !v.iter().any(|x| x.0 == s.name) {
            v.push((s.name, s.cats));
        }
    }
    v
}

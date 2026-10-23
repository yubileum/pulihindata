//! Identity for signature results: carved files have no original name, so we
//! read what the content itself says — a document's title (or first line),
//! a song's tags, when a photo was taken or a video recorded. That makes a
//! deep-scan hit findable by name and date like a filesystem result.

use memchr::memmem;

use super::formats::Ctx;
use crate::bytes::{days_from_civil, Bytes};

#[derive(Clone, Debug, Default)]
pub struct Meta {
    /// Unix seconds (taken / recorded / last saved).
    pub time: Option<i64>,
    /// A name for the file (document title, "Artist - Title").
    pub title: Option<String>,
    /// Extra detail shown to the user (camera model, first words…).
    pub info: Option<String>,
}

pub fn describe(c: &mut Ctx, ext: &str, size: u64) -> Meta {
    let m = match ext {
        "jpg" => jpeg(c),
        "tif" | "cr2" | "nef" | "arw" | "dng" | "orf" | "rw2" | "pef" | "srw" => {
            let b = c.b(0, 256 << 10).to_vec();
            Some(exif_tiff(&b))
        }
        "mp4" | "mov" | "m4v" | "m4a" | "3gp" | "3g2" | "f4v" | "cr3" => mp4(c, size),
        "mp3" => id3(c),
        "flac" => flac(c),
        "docx" | "xlsx" | "pptx" | "vsdx" | "odt" | "ods" | "odp" | "odg" => {
            office(c, size, 0).map(|o| o.0)
        }
        "pdf" => Some(pdf(c, size)),
        _ => None,
    };
    m.unwrap_or_default()
}

fn civil(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Option<i64> {
    let ok = (1990..2100).contains(&y)
        && (1..=12).contains(&mo)
        && (1..=31).contains(&d)
        && (0..24).contains(&h)
        && (0..60).contains(&mi)
        && (0..61).contains(&s);
    ok.then(|| days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s)
}

/// Pull the digit runs out of "2024:03:15 10:30:00", "2024-03-15T10:30:00Z",
/// "D:20240315103000+07'00'" and turn them into a timestamp.
fn parse_time(s: &str) -> Option<i64> {
    let digits: String = s
        .chars()
        .take_while(|c| !matches!(c, '+' | 'Z' | '\''))
        .filter(char::is_ascii_digit)
        .collect();
    if digits.len() < 8 {
        return None;
    }
    let n = |a: usize, b: usize| {
        digits
            .get(a..b)
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(0)
    };
    civil(n(0, 4), n(4, 6), n(6, 8), n(8, 10), n(10, 12), n(12, 14))
}

/// Single-line, trimmed, length-limited text suitable as a file name.
fn tidy(s: &str, max: usize) -> Option<String> {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_matches(|c: char| c.is_control() || c == '\u{FEFF}' || c.is_whitespace());
    // Damaged or binary data must not turn into a garbage file name.
    let total = s.chars().count();
    let clean = s
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .count();
    if s.chars().filter(|c| c.is_alphanumeric()).count() < 2
        || s.contains('\u{FFFD}')
        || s.chars().any(char::is_control)
        || clean * 10 < total * 7
    {
        return None;
    }
    if s.chars().count() <= max {
        return Some(s.to_string());
    }
    let cut: String = s.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > max / 2 => &cut[..i],
        _ => &cut,
    };
    Some(
        cut.trim_end_matches([',', '.', ';', ':', '-', ' '])
            .to_string(),
    )
}

// ------------------------------------------------------------- photos

fn jpeg(c: &mut Ctx) -> Option<Meta> {
    let mut pos = 2u64;
    for _ in 0..32 {
        let s = c.arr::<4>(pos)?;
        if s[0] != 0xFF || s[1] == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([s[2], s[3]]) as u64;
        if s[1] == 0xE1 && len > 8 {
            let seg = c.b(pos + 4, (len - 2) as usize).to_vec();
            if let Some(tiff) = seg.strip_prefix(b"Exif\0\0") {
                return Some(exif_tiff(tiff));
            }
        }
        pos += 2 + len;
    }
    None
}

/// Date taken and camera from a TIFF/EXIF structure.
fn exif_tiff(b: &[u8]) -> Meta {
    let mut m = Meta::default();
    if b.len() < 8 || !(b.starts_with(b"II") || b.starts_with(b"MM")) {
        return m;
    }
    let le = b[0] == b'I';
    let r16 = |o: usize| if le { b.u16le(o) } else { b.u16be(o) };
    let r32 = |o: usize| if le { b.u32le(o) } else { b.u32be(o) };
    let ascii = |at: usize, cnt: usize| -> Option<String> {
        let off = if cnt <= 4 { at } else { r32(at)? as usize };
        let raw = b.at(off, cnt.min(256))?;
        let end = raw.iter().position(|&x| x == 0).unwrap_or(raw.len());
        let s = String::from_utf8_lossy(&raw[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    let (mut make, mut model, mut date, mut original) = (None, None, None, None);
    let mut exif_ifd = None;
    let Some(ifd0) = r32(4) else { return m };
    for (ifd, is_exif) in [(ifd0 as usize, false), (0, true)] {
        let ifd = if is_exif {
            match exif_ifd {
                Some(o) => o,
                None => break,
            }
        } else {
            ifd
        };
        let Some(n) = r16(ifd) else { break };
        for k in 0..(n as usize).min(512) {
            let e = ifd + 2 + k * 12;
            let (Some(tag), Some(cnt)) = (r16(e), r32(e + 4)) else {
                break;
            };
            let cnt = cnt as usize;
            match (is_exif, tag) {
                (false, 0x010F) => make = ascii(e + 8, cnt),
                (false, 0x0110) => model = ascii(e + 8, cnt),
                (false, 0x0132) => date = ascii(e + 8, cnt),
                (false, 0x8769) => exif_ifd = r32(e + 8).map(|v| v as usize),
                (true, 0x9003) => original = ascii(e + 8, cnt),
                _ => {}
            }
        }
    }
    m.time = original.or(date).as_deref().and_then(parse_time);
    m.info = match (make, model) {
        (Some(mk), Some(md))
            if md
                .to_lowercase()
                .starts_with(&mk.split_whitespace().next().unwrap_or("").to_lowercase()) =>
        {
            Some(md)
        }
        (Some(mk), Some(md)) => Some(format!("{mk} {md}")),
        (a, b) => a.or(b),
    };
    m
}

// ------------------------------------------------------------- video

fn mp4(c: &mut Ctx, size: u64) -> Option<Meta> {
    let mut pos = 0u64;
    for _ in 0..64 {
        if pos + 8 > size {
            break;
        }
        let hd = c.b(pos, 16).to_vec();
        let mut bsize = hd.u32be(0)? as u64;
        let mut hlen = 8;
        if bsize == 1 {
            bsize = hd.u64be(8)?;
            hlen = 16;
        }
        if bsize < 8 {
            break;
        }
        if hd.get(4..8)? == b"moov" {
            let mut p = pos + hlen;
            for _ in 0..16 {
                let ch = c.b(p, 24).to_vec();
                let csize = ch.u32be(0)? as u64;
                if ch.get(4..8)? == b"mvhd" {
                    let t = if ch[8] == 1 {
                        ch.u64be(12)?
                    } else {
                        ch.u32be(12)? as u64
                    };
                    // Seconds since 1904-01-01.
                    let unix = t as i64 - 2_082_844_800;
                    let ok = (631_152_000..4_102_444_800).contains(&unix);
                    let local = unix + crate::bytes::local_offset();
                    return Some(Meta {
                        time: ok.then_some(local),
                        ..Default::default()
                    });
                }
                if csize < 8 {
                    break;
                }
                p += csize;
            }
            break;
        }
        pos += bsize;
    }
    None
}

// ------------------------------------------------------------- audio

fn id3_text(d: &[u8]) -> Option<String> {
    let (&enc, body) = d.split_first()?;
    let s = match enc {
        0 => body.iter().map(|&b| b as char).collect::<String>(),
        1 | 2 => {
            let (be, b) = match body {
                [0xFE, 0xFF, rest @ ..] => (true, rest),
                [0xFF, 0xFE, rest @ ..] => (false, rest),
                _ => (enc == 2, body),
            };
            let units: Vec<u16> = b
                .chunks_exact(2)
                .map(|x| {
                    if be {
                        u16::from_be_bytes([x[0], x[1]])
                    } else {
                        u16::from_le_bytes([x[0], x[1]])
                    }
                })
                .take_while(|&u| u != 0)
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(body).into_owned(),
    };
    tidy(s.trim_end_matches('\0'), 120)
}

fn id3(c: &mut Ctx) -> Option<Meta> {
    let h = c.arr::<10>(0)?;
    let ver = h[3];
    let size = h[6..10].iter().fold(0usize, |a, &x| a << 7 | x as usize);
    let tag = c.b(10, size.min(512 << 10)).to_vec();
    let (mut title, mut artist, mut album) = (None, None, None);
    let mut p = 0usize;
    while p + 10 <= tag.len() {
        let (id, fsz, hl) = if ver >= 3 {
            let s = if ver == 4 {
                tag[p + 4..p + 8]
                    .iter()
                    .fold(0usize, |a, &x| a << 7 | (x & 0x7F) as usize)
            } else {
                tag.u32be(p + 4)? as usize
            };
            (&tag[p..p + 4], s, 10)
        } else {
            let s = (tag[p + 3] as usize) << 16 | (tag[p + 4] as usize) << 8 | tag[p + 5] as usize;
            (&tag[p..p + 3], s, 6)
        };
        if id[0] == 0 || fsz == 0 {
            break;
        }
        let data = tag.get(p + hl..p + hl + fsz).unwrap_or(&[]);
        match id {
            b"TIT2" | b"TT2" => title = id3_text(data),
            b"TPE1" | b"TP1" => artist = id3_text(data),
            b"TALB" | b"TAL" => album = id3_text(data),
            _ => {}
        }
        p += hl + fsz;
    }
    Some(song(title, artist, album))
}

fn song(title: Option<String>, artist: Option<String>, album: Option<String>) -> Meta {
    let title = match (&artist, title) {
        (Some(a), Some(t)) => Some(format!("{a} - {t}")),
        (_, t) => t,
    };
    Meta {
        time: None,
        title,
        info: album,
    }
}

fn flac(c: &mut Ctx) -> Option<Meta> {
    let mut pos = 4u64;
    for _ in 0..32 {
        let bh = c.arr::<4>(pos)?;
        let len = (bh[1] as usize) << 16 | (bh[2] as usize) << 8 | bh[3] as usize;
        if bh[0] & 0x7F == 4 {
            let b = c.b(pos + 4, len.min(256 << 10)).to_vec();
            let vlen = b.u32le(0)? as usize;
            let mut p = 4 + vlen;
            let n = b.u32le(p)?;
            p += 4;
            let (mut title, mut artist, mut album) = (None, None, None);
            for _ in 0..n.min(256) {
                let l = b.u32le(p)? as usize;
                let kv = String::from_utf8_lossy(b.at(p + 4, l)?).into_owned();
                p += 4 + l;
                if let Some((k, v)) = kv.split_once('=') {
                    match k.to_ascii_uppercase().as_str() {
                        "TITLE" => title = tidy(v, 120),
                        "ARTIST" => artist = tidy(v, 80),
                        "ALBUM" => album = tidy(v, 80),
                        _ => {}
                    }
                }
            }
            return Some(song(title, artist, album));
        }
        if bh[0] & 0x80 != 0 {
            break;
        }
        pos += 4 + len as u64;
    }
    None
}

// ------------------------------------------------------------- documents

/// Read one ZIP member (stored or deflated), at most `limit` bytes of output.
fn zip_member(c: &mut Ctx, lho: u64, method: u16, csize: u64, limit: usize) -> Option<Vec<u8>> {
    let h = c.arr::<30>(lho)?;
    if h[0..4] != *b"PK\x03\x04" {
        return None;
    }
    let data = lho + 30 + h.u16le(26)? as u64 + h.u16le(28)? as u64;
    let raw = c.b(data, csize.min(4 << 20) as usize).to_vec();
    match method {
        0 => Some(raw[..raw.len().min(limit)].to_vec()),
        8 => Some(
            match miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, limit) {
                Ok(v) => v,
                Err(e) => e.output, // partial output is plenty for a title
            },
        ),
        _ => None,
    }
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';').filter(|&j| j < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=j]),
        }
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    out
}

/// Text content of the first `<tag …>…</tag>` (tag given without '<').
fn xml_value(x: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = x[from..].find(&open) {
        let at = from + i + open.len();
        // Must be the whole tag name, not a prefix of a longer one.
        if x[at..].starts_with(['>', ' ', '/']) {
            let gt = at + x[at..].find('>')?;
            if x[..gt].ends_with('/') {
                return None;
            }
            let end = gt + 1 + x[gt + 1..].find(&format!("</{tag}"))?;
            return Some(xml_unescape(&x[gt + 1..end]));
        }
        from = at;
    }
    None
}

/// Readable text of an Office/ODF document (for previews), at most
/// `max_chars` characters, paragraphs on separate lines.
pub fn document_text(c: &mut Ctx, ext: &str, size: u64, max_chars: usize) -> Option<String> {
    match ext {
        "docx" | "xlsx" | "pptx" | "vsdx" | "odt" | "ods" | "odp" | "odg" => {
            office(c, size, max_chars)?.1
        }
        _ => None,
    }
}

/// Visible text of an XML body: tags dropped, paragraphs separated by `sep`.
fn xml_text(x: &str, max_chars: usize) -> String {
    xml_text_sep(x, max_chars, ' ')
}

fn xml_text_sep(x: &str, max_chars: usize, sep: char) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in x.chars() {
        match (in_tag, ch) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                if tag.starts_with("/w:p")
                    || tag.starts_with("/a:p")
                    || tag.starts_with("/text:p")
                    || tag.starts_with("/text:h")
                    || tag.starts_with("/si")
                {
                    out.push(sep);
                } else if tag.starts_with("w:tab")
                    || tag.starts_with("text:s")
                    || tag.starts_with("/c")
                {
                    out.push(' ');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
        if out.len() > max_chars * 4 {
            break;
        }
    }
    xml_unescape(&out)
}

/// Title/date of an Office/ODF file, plus (when `text_chars` > 0) its text.
fn office(c: &mut Ctx, size: u64, text_chars: usize) -> Option<(Meta, Option<String>)> {
    // Central directory via the end record (with or without a comment).
    let tail_len = size.min(66_000);
    let tail = c.b(size - tail_len, tail_len as usize).to_vec();
    let e = memmem::rfind(&tail, b"PK\x05\x06")?;
    let cd_size = tail.u32le(e + 12)? as u64;
    let cd_off = tail.u32le(e + 16)? as u64;
    if cd_off.checked_add(cd_size)? > size {
        return None;
    }
    let cd = c.b(cd_off, cd_size.min(8 << 20) as usize).to_vec();
    let mut entries: Vec<(String, u16, u64, u64)> = Vec::new();
    let mut p = 0usize;
    while p + 46 <= cd.len() && cd[p..p + 4] == *b"PK\x01\x02" {
        let method = cd.u16le(p + 10)?;
        let csize = cd.u32le(p + 20)? as u64;
        let (n, x, k) = (
            cd.u16le(p + 28)? as usize,
            cd.u16le(p + 30)? as usize,
            cd.u16le(p + 32)? as usize,
        );
        let lho = cd.u32le(p + 42)? as u64;
        let name = String::from_utf8_lossy(cd.at(p + 46, n)?).into_owned();
        entries.push((name, method, csize, lho));
        p += 46 + n + x + k;
    }
    let mut get = |want: &[&str], limit: usize| -> Option<String> {
        let (_, m, cs, lho) = entries
            .iter()
            .find(|e| want.contains(&e.0.as_str()))?
            .clone();
        let v = zip_member(c, lho, m, cs, limit)?;
        Some(String::from_utf8_lossy(&v).into_owned())
    };
    let mut meta = Meta::default();
    if let Some(core) = get(&["docProps/core.xml", "meta.xml"], 64 << 10) {
        meta.title = xml_value(&core, "dc:title").and_then(|t| tidy(&t, 120));
        let stamp = xml_value(&core, "dcterms:modified")
            .or_else(|| xml_value(&core, "dc:date"))
            .or_else(|| xml_value(&core, "dcterms:created"));
        // Office writes UTC ("…Z"); show it in local time like everything else.
        let utc = stamp
            .as_deref()
            .is_some_and(|s| s.trim_end().ends_with('Z'));
        meta.time = stamp.as_deref().and_then(parse_time).map(|t| {
            if utc {
                t + crate::bytes::local_offset()
            } else {
                t
            }
        });
    }
    let body = get(
        &[
            "word/document.xml",
            "ppt/slides/slide1.xml",
            "xl/sharedStrings.xml",
            "content.xml",
            "visio/pages/page1.xml",
        ],
        if text_chars > 0 { 1 << 20 } else { 128 << 10 },
    );
    let body_start = |b: &str| {
        b.find("<w:body")
            .or_else(|| b.find("<office:body"))
            .unwrap_or(0)
    };
    let text = (text_chars > 0)
        .then_some(body.as_deref())
        .flatten()
        .map(|b| {
            let t = xml_text_sep(&b[body_start(b)..], text_chars, '\n');
            let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            let mut out = lines.join("\n");
            if let Some((i, _)) = out.char_indices().nth(text_chars) {
                out.truncate(i);
            }
            out
        })
        .filter(|t| !t.is_empty());
    // The opening words identify a document even when it has no title.
    let snippet = body.map(|b| xml_text(&b[body_start(&b)..], 200));
    let snippet = snippet.as_deref().and_then(|s| tidy(s, 48));
    if meta.title.is_none() {
        meta.title = snippet.clone();
    } else {
        meta.info = snippet;
    }
    Some((meta, text))
}

/// A PDF string object at the start of `s` ("(literal)" or "<hex>").
fn pdf_string(s: &[u8]) -> Option<String> {
    let s = s.trim_ascii_start();
    let bytes: Vec<u8> = match s.first()? {
        b'(' => {
            let mut out = Vec::new();
            let mut depth = 0;
            let mut i = 1;
            while i < s.len() && out.len() < 1024 {
                let ch = s[i];
                match ch {
                    b'\\' => {
                        i += 1;
                        let e = *s.get(i)?;
                        match e {
                            b'n' => out.push(b'\n'),
                            b'r' | b't' | b'b' | b'f' => out.push(b' '),
                            b'0'..=b'7' => {
                                let mut v = 0u32;
                                let mut k = 0;
                                while k < 3
                                    && s.get(i + k).is_some_and(|d| (b'0'..=b'7').contains(d))
                                {
                                    v = v * 8 + (s[i + k] - b'0') as u32;
                                    k += 1;
                                }
                                out.push(v as u8);
                                i += k - 1;
                            }
                            b'\r' | b'\n' => {}
                            other => out.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        out.push(ch);
                    }
                    b')' if depth == 0 => break,
                    b')' => {
                        depth -= 1;
                        out.push(ch);
                    }
                    _ => out.push(ch),
                }
                i += 1;
            }
            out
        }
        b'<' => {
            let end = s.iter().position(|&b| b == b'>')?;
            let hex: Vec<u8> = s[1..end]
                .iter()
                .copied()
                .filter(u8::is_ascii_hexdigit)
                .collect();
            hex.chunks(2)
                .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
                .collect()
        }
        _ => return None,
    };
    Some(match bytes.as_slice() {
        [0xFE, 0xFF, rest @ ..] => {
            let u: Vec<u16> = rest
                .chunks_exact(2)
                .map(|x| u16::from_be_bytes([x[0], x[1]]))
                .collect();
            String::from_utf16_lossy(&u)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        b => b.iter().map(|&x| x as char).collect(),
    })
}

fn clean_pdf_title(t: &str) -> String {
    let mut t = t.trim();
    for p in [
        "Microsoft Word - ",
        "Microsoft PowerPoint - ",
        "Microsoft Excel - ",
        "Microsoft Word – ",
    ] {
        if let Some(r) = t.strip_prefix(p) {
            t = r;
        }
    }
    for s in [
        ".docx", ".doc", ".pptx", ".ppt", ".xlsx", ".xls", ".odt", ".rtf",
    ] {
        if t.len() > s.len() && t.to_ascii_lowercase().ends_with(s) {
            t = &t[..t.len() - s.len()];
        }
    }
    t.to_string()
}

fn pdf(c: &mut Ctx, size: u64) -> Meta {
    let mut m = Meta::default();
    let tail_len = size.min(64 << 10);
    let tail = c.b(size - tail_len, tail_len as usize).to_vec();
    let head = c.b(0, (64u64 << 10).min(size) as usize).to_vec();
    for buf in [&tail, &head] {
        if m.title.is_none() {
            if let Some(i) = memmem::rfind(buf, b"/Title") {
                m.title = pdf_string(&buf[i + 6..])
                    .map(|t| clean_pdf_title(&t))
                    .and_then(|t| tidy(&t, 120));
            }
        }
        if m.time.is_none() {
            if let Some(i) =
                memmem::rfind(buf, b"/ModDate").or_else(|| memmem::rfind(buf, b"/CreationDate"))
            {
                let skip = if buf[i..].starts_with(b"/ModDate") {
                    8
                } else {
                    13
                };
                m.time = pdf_string(&buf[i + skip..]).as_deref().and_then(parse_time);
            }
        }
    }
    // XMP packet (plain XML) as a fallback.
    if m.title.is_none() || m.time.is_none() {
        for buf in [&head, &tail] {
            let x = String::from_utf8_lossy(buf);
            if m.title.is_none() {
                if let Some(t) = xml_value(&x, "dc:title") {
                    m.title = xml_value(&t, "rdf:li")
                        .map(|t| clean_pdf_title(&t))
                        .and_then(|t| tidy(&t, 120));
                }
            }
            if m.time.is_none() {
                m.time = xml_value(&x, "xmp:ModifyDate")
                    .as_deref()
                    .and_then(parse_time);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time("2023:11:14 22:13:20"), Some(1_700_000_000));
        assert_eq!(parse_time("2023-11-14T22:13:20+00:00"), Some(1_700_000_000));
        assert_eq!(parse_time("D:20231114221320+07'00'"), Some(1_700_000_000));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn xml() {
        let core = r#"<cp:coreProperties><dc:title>Skripsi &amp; Lampiran</dc:title><dcterms:modified xsi:type="x">2024-03-15T10:30:00Z</dcterms:modified></cp:coreProperties>"#;
        assert_eq!(
            xml_value(core, "dc:title").as_deref(),
            Some("Skripsi & Lampiran")
        );
        let body = "<w:body><w:p><w:r><w:t>BAB I</w:t></w:r></w:p><w:p><w:t>PENDAHULUAN</w:t></w:p></w:body>";
        assert_eq!(
            tidy(&xml_text(body, 100), 70).as_deref(),
            Some("BAB I PENDAHULUAN")
        );
    }

    #[test]
    fn pdf_strings() {
        assert_eq!(
            pdf_string(b" (Microsoft Word - skripsi.docx)")
                .map(|t| clean_pdf_title(&t))
                .as_deref(),
            Some("skripsi")
        );
        assert_eq!(pdf_string(b"<FEFF0048 0069>").as_deref(), Some("Hi"));
        assert_eq!(pdf_string(br"(a\(b\)c \101)").as_deref(), Some("a(b)c A"));
    }

    #[test]
    fn tidy_names() {
        assert_eq!(tidy("  a\n b  ", 10).as_deref(), Some("a b"));
        assert_eq!(tidy("…", 10), None);
        assert_eq!(tidy("x\u{FFFD}#%&*!@zz", 20), None);
        assert_eq!(
            tidy("satu dua tiga empat lima", 12).as_deref(),
            Some("satu dua")
        );
    }
}

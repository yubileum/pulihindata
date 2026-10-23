//! Bounds-checked little/big-endian readers. Every parser in this crate runs on
//! untrusted (possibly half-overwritten) disk data, so nothing indexes raw.

pub trait Bytes {
    fn u8_at(&self, o: usize) -> Option<u8>;
    fn u16le(&self, o: usize) -> Option<u16>;
    fn u32le(&self, o: usize) -> Option<u32>;
    fn u64le(&self, o: usize) -> Option<u64>;
    fn u16be(&self, o: usize) -> Option<u16>;
    fn u32be(&self, o: usize) -> Option<u32>;
    fn u64be(&self, o: usize) -> Option<u64>;
    fn at(&self, o: usize, n: usize) -> Option<&[u8]>;
}

impl Bytes for [u8] {
    #[inline]
    fn u8_at(&self, o: usize) -> Option<u8> {
        self.get(o).copied()
    }
    #[inline]
    fn u16le(&self, o: usize) -> Option<u16> {
        Some(u16::from_le_bytes(
            self.get(o..o.checked_add(2)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn u32le(&self, o: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.get(o..o.checked_add(4)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn u64le(&self, o: usize) -> Option<u64> {
        Some(u64::from_le_bytes(
            self.get(o..o.checked_add(8)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn u16be(&self, o: usize) -> Option<u16> {
        Some(u16::from_be_bytes(
            self.get(o..o.checked_add(2)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn u32be(&self, o: usize) -> Option<u32> {
        Some(u32::from_be_bytes(
            self.get(o..o.checked_add(4)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn u64be(&self, o: usize) -> Option<u64> {
        Some(u64::from_be_bytes(
            self.get(o..o.checked_add(8)?)?.try_into().ok()?,
        ))
    }
    #[inline]
    fn at(&self, o: usize, n: usize) -> Option<&[u8]> {
        self.get(o..o.checked_add(n)?)
    }
}

/// Decode UTF-16LE (as used by NTFS / exFAT / FAT LFN), stopping at NUL.
pub fn utf16le(b: &[u8]) -> String {
    let units = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0);
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

const fn crc32_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC32: [u32; 256] = crc32_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC32[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// Seconds the local time zone is ahead of UTC (WIB = +25200).
///
/// Every `modified` time in this crate is *local wall-clock* time, the way
/// FAT and EXIF store it and the way people remember it; sources that record
/// UTC (NTFS, MP4, Office) are shifted by this offset.
pub fn local_offset() -> i64 {
    static OFF: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        #[cfg(unix)]
        // SAFETY: localtime_r only writes into the provided struct.
        unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                0
            } else {
                tm.tm_gmtoff as i64
            }
        }
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetTimeZoneInformation(tzi: *mut u8) -> u32;
            }
            let mut tzi = [0u8; 172];
            // SAFETY: TIME_ZONE_INFORMATION is 172 bytes.
            let id = unsafe { GetTimeZoneInformation(tzi.as_mut_ptr()) };
            let rd = |o: usize| i32::from_le_bytes(tzi[o..o + 4].try_into().unwrap()) as i64;
            let extra = if id == 2 { rd(168) } else { rd(84) };
            -(rd(0) + extra) * 60
        }
        #[cfg(not(any(unix, windows)))]
        0
    })
}

/// Convert a Windows FILETIME (100ns ticks since 1601) to Unix seconds.
pub fn filetime_to_unix(ft: u64) -> Option<i64> {
    if ft == 0 {
        return None;
    }
    let secs = (ft / 10_000_000) as i64 - 11_644_473_600;
    // Reject garbage outside 1980..2100.
    (315_532_800..4_102_444_800).contains(&secs).then_some(secs)
}

/// Convert DOS date/time (FAT, exFAT) to Unix seconds (treated as UTC).
pub fn dos_to_unix(date: u16, time: u16) -> Option<i64> {
    let year = 1980 + (date >> 9) as i64;
    let month = ((date >> 5) & 0xF) as i64;
    let day = (date & 0x1F) as i64;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let h = (time >> 11) as i64;
    let m = ((time >> 5) & 0x3F) as i64;
    let s = ((time & 0x1F) * 2) as i64;
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Format Unix seconds as `YYYY-MM-DD HH:MM` (UTC).
pub fn fmt_time(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem / 60) % 60
    )
}

/// Human-readable byte size.
pub fn fmt_size(n: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", U[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", U[i])
    } else {
        format!("{v:.2} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn time() {
        assert_eq!(fmt_time(0), "1970-01-01 00:00");
        assert_eq!(fmt_time(1_700_000_000), "2023-11-14 22:13");
        let t = dos_to_unix((44 << 9) | (3 << 5) | 15, (10 << 11) | (30 << 5)).unwrap();
        assert_eq!(fmt_time(t), "2024-03-15 10:30");
    }
}

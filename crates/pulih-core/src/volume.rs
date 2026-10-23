//! Partition tables (MBR / GPT) and filesystem detection.

use crate::bytes::{utf16le, Bytes};
use crate::device::{read_vec, ReadAt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FsType {
    Ntfs,
    Fat12,
    Fat16,
    Fat32,
    ExFat,
    Ext,
    HfsPlus,
    Apfs,
    Unknown,
}

impl FsType {
    pub fn name(self) -> &'static str {
        match self {
            FsType::Ntfs => "NTFS",
            FsType::Fat12 => "FAT12",
            FsType::Fat16 => "FAT16",
            FsType::Fat32 => "FAT32",
            FsType::ExFat => "exFAT",
            FsType::Ext => "ext2/3/4",
            FsType::HfsPlus => "HFS+",
            FsType::Apfs => "APFS",
            FsType::Unknown => "Unknown",
        }
    }

    /// Filesystems whose deleted-file metadata this crate can read.
    pub fn supported(self) -> bool {
        matches!(
            self,
            FsType::Ntfs | FsType::Fat12 | FsType::Fat16 | FsType::Fat32 | FsType::ExFat
        )
    }
}

#[derive(Clone, Debug)]
pub struct Volume {
    pub index: u16,
    /// Absolute byte offset on the device.
    pub offset: u64,
    pub size: u64,
    pub fs: FsType,
    pub desc: String,
}

/// Classify a FAT boot sector, returning the FAT width by cluster count.
pub(crate) fn fat_kind(b: &[u8]) -> Option<FsType> {
    let bps = b.u16le(11)? as u64;
    let spc = b.u8_at(13)? as u64;
    let reserved = b.u16le(14)? as u64;
    let nfats = b.u8_at(16)? as u64;
    let root_ents = b.u16le(17)? as u64;
    let media = b.u8_at(21)?;
    if b.u16le(510)? != 0xAA55
        || !matches!(bps, 512 | 1024 | 2048 | 4096)
        || spc == 0
        || !spc.is_power_of_two()
        || reserved == 0
        || !(1..=2).contains(&nfats)
        || !(media == 0xF0 || media >= 0xF8)
    {
        return None;
    }
    let tot16 = b.u16le(19)? as u64;
    let tot = if tot16 != 0 {
        tot16
    } else {
        b.u32le(32)? as u64
    };
    let fsz16 = b.u16le(22)? as u64;
    let fsz = if fsz16 != 0 {
        fsz16
    } else {
        b.u32le(36)? as u64
    };
    if fsz == 0 || tot == 0 {
        return None;
    }
    let root_secs = (root_ents * 32).div_ceil(bps);
    let data_start = reserved + nfats * fsz + root_secs;
    let clusters = tot.checked_sub(data_start)? / spc;
    Some(if clusters < 4085 {
        FsType::Fat12
    } else if clusters < 65525 {
        FsType::Fat16
    } else {
        FsType::Fat32
    })
}

pub fn detect_fs(src: &dyn ReadAt, off: u64) -> FsType {
    let Ok(b) = read_vec(src, off, 4096) else {
        return FsType::Unknown;
    };
    if b.len() < 512 {
        return FsType::Unknown;
    }
    if &b[3..11] == b"NTFS    " {
        return FsType::Ntfs;
    }
    if &b[3..11] == b"EXFAT   " {
        return FsType::ExFat;
    }
    if let Some(k) = fat_kind(&b) {
        return k;
    }
    if b.len() >= 2048 {
        if b.u16le(1080) == Some(0xEF53) {
            return FsType::Ext;
        }
        if &b[1024..1026] == b"H+" || &b[1024..1026] == b"HX" {
            return FsType::HfsPlus;
        }
    }
    if &b[32..36] == b"NXSB" {
        return FsType::Apfs;
    }
    FsType::Unknown
}

/// Find all volumes on a source. A source that is itself a volume (e.g.
/// `\\.\E:` or `/dev/sdb1`) yields exactly one entry at offset 0.
pub fn find_volumes(src: &dyn ReadAt) -> Vec<Volume> {
    let size = src.size();
    let fs0 = detect_fs(src, 0);
    if fs0 != FsType::Unknown {
        return vec![Volume {
            index: 0,
            offset: 0,
            size,
            fs: fs0,
            desc: fs0.name().into(),
        }];
    }
    let mut parts: Vec<(u64, u64, String)> = Vec::new();
    let mut gpt_found = false;
    for ss in [512u64, 4096] {
        if let Some(p) = parse_gpt(src, ss) {
            parts = p;
            gpt_found = true;
            break;
        }
    }
    if !gpt_found {
        parts = parse_mbr(src);
    }
    let mut out = Vec::new();
    for (start, len, name) in parts {
        if start >= size || len == 0 {
            continue;
        }
        let len = len.min(size - start);
        let fs = detect_fs(src, start);
        let idx = out.len() as u16;
        let mut desc = format!("Partition {} ({})", idx + 1, fs.name());
        if !name.is_empty() {
            desc.push_str(&format!(" - {name}"));
        }
        out.push(Volume {
            index: idx,
            offset: start,
            size: len,
            fs,
            desc,
        });
    }
    out
}

fn parse_gpt(src: &dyn ReadAt, ss: u64) -> Option<Vec<(u64, u64, String)>> {
    let h = read_vec(src, ss, 512).ok()?;
    if h.get(0..8)? != b"EFI PART" {
        return None;
    }
    let lba = h.u64le(72)?;
    let count = (h.u32le(80)? as u64).min(1024);
    let esz = h.u32le(84)? as u64;
    if !(128..=4096).contains(&esz) {
        return None;
    }
    let table = read_vec(src, lba.checked_mul(ss)?, (count * esz) as usize).ok()?;
    let mut out = Vec::new();
    for i in 0..count as usize {
        let e = table.at(i * esz as usize, esz as usize)?;
        if e[0..16].iter().all(|&b| b == 0) {
            continue;
        }
        let first = e.u64le(32)?;
        let last = e.u64le(40)?;
        if last < first {
            continue;
        }
        let name = utf16le(e.at(56, 72).unwrap_or(&[]));
        out.push((first * ss, (last - first + 1) * ss, name));
    }
    Some(out)
}

fn parse_mbr(src: &dyn ReadAt) -> Vec<(u64, u64, String)> {
    let mut out = Vec::new();
    let Ok(b) = read_vec(src, 0, 512) else {
        return out;
    };
    if b.u16le(510) != Some(0xAA55) {
        return out;
    }
    for i in 0..4 {
        let e = &b[446 + i * 16..462 + i * 16];
        let ty = e[4];
        let start = e.u32le(8).unwrap_or(0) as u64;
        let len = e.u32le(12).unwrap_or(0) as u64;
        if ty == 0 || len == 0 {
            continue;
        }
        if matches!(ty, 0x05 | 0x0F | 0x85) {
            // Walk the extended-partition chain of EBRs.
            let ext_base = start;
            let mut ebr = start;
            for _ in 0..128 {
                let Ok(eb) = read_vec(src, ebr * 512, 512) else {
                    break;
                };
                if eb.u16le(510) != Some(0xAA55) {
                    break;
                }
                let l = &eb[446..462];
                let (ls, ll) = (
                    l.u32le(8).unwrap_or(0) as u64,
                    l.u32le(12).unwrap_or(0) as u64,
                );
                if l[4] != 0 && ll != 0 {
                    out.push(((ebr + ls) * 512, ll * 512, String::new()));
                }
                let n = &eb[462..478];
                let next = n.u32le(8).unwrap_or(0) as u64;
                if n[4] == 0 || next == 0 {
                    break;
                }
                ebr = ext_base + next;
            }
        } else {
            out.push((start * 512, len * 512, String::new()));
        }
    }
    out.sort_by_key(|p| p.0);
    out
}

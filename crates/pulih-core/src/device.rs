//! Raw, read-only access to disks, partitions and disk-image files.
//!
//! Everything is positional (`pread` / `ReadFile` with an offset) so many
//! threads can share one handle, and the source is never opened for writing.

use std::alloc::{self, Layout};
use std::fs::File;
use std::io;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

pub const SECTOR: u64 = 512;
const BUF_ALIGN: usize = 4096;

pub trait ReadAt: Send + Sync {
    /// Read up to `buf.len()` bytes at `off`. Returns 0 at end of device.
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize>;
    fn size(&self) -> u64;
}

/// Read until `buf` is full or the device ends. Returns bytes read.
pub fn read_full(src: &dyn ReadAt, mut off: u64, mut buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while !buf.is_empty() {
        match src.read_at(off, buf) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                off += n as u64;
                buf = &mut buf[n..];
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// Read `n` bytes at `off` into a new vector (shorter at end of device).
pub fn read_vec(src: &dyn ReadAt, off: u64, n: usize) -> io::Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    let got = read_full(src, off, &mut v)?;
    v.truncate(got);
    Ok(v)
}

/// Read that survives bad sectors: unreadable sectors are zero-filled.
/// Returns `(bytes_read, bad_sectors)`.
pub fn read_resilient(src: &dyn ReadAt, off: u64, buf: &mut [u8]) -> (usize, u64) {
    if let Ok(n) = read_full(src, off, buf) {
        return (n, 0);
    }
    // Retry in progressively smaller pieces so one bad sector costs 512 bytes,
    // not a whole chunk.
    let end = (off + buf.len() as u64).min(src.size());
    let len = end.saturating_sub(off) as usize;
    let mut bad = 0;
    let mut pos = 0usize;
    while pos < len {
        let piece = (64 * 1024).min(len - pos);
        if read_full(src, off + pos as u64, &mut buf[pos..pos + piece]).is_err() {
            let mut s = pos;
            while s < pos + piece {
                let n = (SECTOR as usize).min(pos + piece - s);
                if read_full(src, off + s as u64, &mut buf[s..s + n]).is_err() {
                    buf[s..s + n].fill(0);
                    bad += 1;
                }
                s += n;
            }
        }
        pos += piece;
    }
    (len, bad)
}

/// Heap buffer aligned for unbuffered device I/O.
pub struct AlignedBuf {
    ptr: NonNull<u8>,
    len: usize,
}

unsafe impl Send for AlignedBuf {}
unsafe impl Sync for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize) -> Self {
        let len = len.max(1);
        let layout = Layout::from_size_align(len, BUF_ALIGN).expect("buffer layout");
        // SAFETY: layout has non-zero size.
        let raw = unsafe { alloc::alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).unwrap_or_else(|| alloc::handle_alloc_error(layout));
        AlignedBuf { ptr, len }
    }
}

impl Deref for AlignedBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        // SAFETY: ptr is valid for len initialised (zeroed) bytes.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: unique borrow of an owned allocation.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.len, BUF_ALIGN).expect("buffer layout");
        // SAFETY: allocated in `new` with the same layout.
        unsafe { alloc::dealloc(self.ptr.as_ptr(), layout) }
    }
}

/// An opened disk, partition or image file.
pub struct Device {
    file: File,
    size: u64,
    /// Offset/length granularity the OS requires (1 for regular files).
    align: u64,
    pub path: String,
    pub is_raw: bool,
}

impl Device {
    pub fn open(path: &str) -> io::Result<Device> {
        let file = open_readonly(path)?;
        let (size, align, is_raw) = probe(&file, path)?;
        Ok(Device {
            file,
            size,
            align,
            path: path.to_string(),
            is_raw,
        })
    }

    #[inline]
    fn pread(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            self.file.read_at(buf, off)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            self.file.seek_read(buf, off)
        }
    }
}

impl ReadAt for Device {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        if off >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let len = (buf.len() as u64).min(self.size - off) as usize;
        let a = self.align;
        let aligned = a == 1
            || (off.is_multiple_of(a)
                && (len as u64).is_multiple_of(a)
                && (buf.as_ptr() as usize).is_multiple_of(BUF_ALIGN));
        if aligned {
            return self.pread(off, &mut buf[..len]);
        }
        // Bounce through an aligned buffer covering whole sectors.
        let start = off / a * a;
        let end = (off + len as u64).div_ceil(a) * a;
        let end = end.min(self.size.div_ceil(a) * a);
        let mut tmp = AlignedBuf::new((end - start) as usize);
        let mut got = 0usize;
        while got < tmp.len() {
            match self.pread(start + got as u64, &mut tmp[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        let skip = (off - start) as usize;
        let n = got.saturating_sub(skip).min(len);
        buf[..n].copy_from_slice(&tmp[skip..skip + n]);
        Ok(n)
    }

    fn size(&self) -> u64 {
        self.size
    }
}

/// In-memory source, used by tests and for previews.
pub struct MemSource(pub Vec<u8>);

impl ReadAt for MemSource {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        let len = self.0.len() as u64;
        if off >= len {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(len - off) as usize;
        buf[..n].copy_from_slice(&self.0[off as usize..off as usize + n]);
        Ok(n)
    }
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

/// A window onto part of another source (e.g. one partition of a disk).
pub struct SubSource<'a> {
    pub inner: &'a dyn ReadAt,
    pub base: u64,
    pub len: u64,
}

impl ReadAt for SubSource<'_> {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        if off >= self.len {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(self.len - off) as usize;
        self.inner.read_at(self.base + off, &mut buf[..n])
    }
    fn size(&self) -> u64 {
        self.len
    }
}

// ---------------------------------------------------------------------------
// Platform specifics
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn open_readonly(path: &str) -> io::Result<File> {
    File::open(path)
}

#[cfg(unix)]
fn probe(file: &File, _path: &str) -> io::Result<(u64, u64, bool)> {
    use std::io::{Seek, SeekFrom};
    use std::os::unix::fs::FileTypeExt;
    let meta = file.metadata()?;
    let ft = meta.file_type();
    let is_raw = ft.is_block_device() || ft.is_char_device();
    #[cfg(target_os = "linux")]
    // SAFETY: plain fd advisory call; failure is harmless.
    unsafe {
        use std::os::fd::AsRawFd;
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_SEQUENTIAL);
    }
    #[cfg(target_os = "macos")]
    if is_raw {
        use std::os::fd::AsRawFd;
        let (mut bs, mut bc) = (0u32, 0u64);
        // SAFETY: DKIOCGETBLOCKSIZE / DKIOCGETBLOCKCOUNT write into the given ints.
        unsafe {
            libc::ioctl(file.as_raw_fd(), 0x4004_6418, &mut bs);
            libc::ioctl(file.as_raw_fd(), 0x4008_6419, &mut bc);
        }
        let bs = if bs == 0 { 512 } else { bs as u64 };
        return Ok((bc * bs, if ft.is_char_device() { bs } else { 1 }, true));
    }
    let size = if is_raw {
        let mut f = file;
        f.seek(SeekFrom::End(0))?
    } else {
        meta.len()
    };
    Ok((size, 1, is_raw))
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    pub type Handle = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn DeviceIoControl(
            h: Handle,
            code: u32,
            inbuf: *const c_void,
            insize: u32,
            outbuf: *mut c_void,
            outsize: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        pub fn GetLogicalDrives() -> u32;
        pub fn GetDriveTypeW(root: *const u16) -> u32;
        pub fn GetVolumeInformationW(
            root: *const u16,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            max_comp: *mut u32,
            flags: *mut u32,
            fs_name: *mut u16,
            fs_name_len: u32,
        ) -> i32;
        pub fn GetDiskFreeSpaceExW(
            dir: *const u16,
            free_caller: *mut u64,
            total: *mut u64,
            free_total: *mut u64,
        ) -> i32;
    }

    pub const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
    pub const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;
    pub const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    pub const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
    pub const FSCTL_ALLOW_EXTENDED_DASD_IO: u32 = 0x0009_0083;

    pub fn ioctl(h: Handle, code: u32, input: &[u8], out: &mut [u8]) -> Option<usize> {
        let mut ret = 0u32;
        // SAFETY: buffers are valid for the given sizes; synchronous call.
        let ok = unsafe {
            DeviceIoControl(
                h,
                code,
                input.as_ptr() as *const c_void,
                input.len() as u32,
                out.as_mut_ptr() as *mut c_void,
                out.len() as u32,
                &mut ret,
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(ret as usize)
    }

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn from_wide(b: &[u16]) -> String {
        let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf16_lossy(&b[..n])
    }
}

#[cfg(windows)]
fn open_readonly(path: &str) -> io::Result<File> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: a mounted
    // volume stays usable by Windows while we read it.
    OpenOptions::new().read(true).share_mode(0x7).open(path)
}

#[cfg(windows)]
fn probe(file: &File, path: &str) -> io::Result<(u64, u64, bool)> {
    use std::os::windows::io::AsRawHandle;
    let is_raw = path.starts_with(r"\\.\") || path.starts_with(r"\\?\GLOBALROOT");
    if !is_raw {
        return Ok((file.metadata()?.len(), 1, false));
    }
    let h = file.as_raw_handle() as win::Handle;
    // Allow reading the whole volume, including sectors past the filesystem.
    let _ = win::ioctl(h, win::FSCTL_ALLOW_EXTENDED_DASD_IO, &[], &mut []);
    let mut geo = [0u8; 256];
    let sector = win::ioctl(h, win::IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geo)
        .map(|_| u32::from_le_bytes(geo[20..24].try_into().unwrap()) as u64)
        .filter(|s| s.is_power_of_two() && (512..=65536).contains(s))
        .unwrap_or(512);
    let mut len = [0u8; 8];
    let size = match win::ioctl(h, win::IOCTL_DISK_GET_LENGTH_INFO, &[], &mut len) {
        Some(_) => u64::from_le_bytes(len),
        None => u64::from_le_bytes(geo[24..32].try_into().unwrap()),
    };
    if size == 0 {
        return Err(io::Error::other("could not determine device size"));
    }
    Ok((size, sector, true))
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Disk,
    Partition,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    /// What to pass to [`Device::open`].
    pub path: String,
    pub name: String,
    pub size: u64,
    pub kind: DeviceKind,
    pub fs: String,
    pub label: String,
    pub mount: String,
    pub removable: bool,
    /// Path of the whole disk this partition lives on.
    pub parent: Option<String>,
    /// Free bytes of a mounted filesystem (for usage bars).
    pub free: Option<u64>,
}

/// List disks and partitions/volumes. Never fails: unreadable entries are
/// simply listed with less detail.
pub fn list_devices() -> Vec<DeviceInfo> {
    #[cfg(target_os = "linux")]
    return linux_list();
    #[cfg(windows)]
    return windows_list();
    #[cfg(target_os = "macos")]
    return macos_list();
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(target_os = "linux")]
fn sniff_fs(path: &str) -> String {
    Device::open(path)
        .ok()
        .map(|d| crate::volume::detect_fs(&d, 0).name().to_string())
        .filter(|s| s != "Unknown")
        .unwrap_or_default()
}

/// Free bytes of the filesystem mounted at `mount`.
#[cfg(unix)]
fn free_space(mount: &str) -> Option<u64> {
    if mount.is_empty() {
        return None;
    }
    let c = std::ffi::CString::new(mount).ok()?;
    // SAFETY: statvfs writes into the zeroed struct we own.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c.as_ptr(), &mut st) == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
    }
}

#[cfg(target_os = "linux")]
fn linux_list() -> Vec<DeviceInfo> {
    use std::collections::HashMap;
    use std::fs;
    let read = |p: String| {
        fs::read_to_string(p)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let mut mounts: HashMap<String, (String, String)> = HashMap::new();
    for line in read("/proc/mounts".into()).lines() {
        let mut it = line.split_whitespace();
        if let (Some(dev), Some(mp), Some(fs)) = (it.next(), it.next(), it.next()) {
            if dev.starts_with("/dev/") {
                let real = fs::canonicalize(dev)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| dev.to_string());
                let mp = mp.replace("\\040", " ");
                mounts.entry(real).or_insert((mp, fs.to_string()));
            }
        }
    }
    let mut out = Vec::new();
    let Ok(dir) = fs::read_dir("/sys/block") else {
        return out;
    };
    let mut names: Vec<String> = dir
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        if name.starts_with("ram") || name.starts_with("zram") {
            continue;
        }
        let sys = format!("/sys/block/{name}");
        let size = read(format!("{sys}/size")).parse::<u64>().unwrap_or(0) * 512;
        if size == 0 {
            continue;
        }
        let model = format!(
            "{} {}",
            read(format!("{sys}/device/vendor")),
            read(format!("{sys}/device/model"))
        );
        let model = model.trim().to_string();
        let removable = read(format!("{sys}/removable")) == "1";
        let path = format!("/dev/{name}");
        let (mount, fstype) = mounts.get(&path).cloned().unwrap_or_default();
        let free = free_space(&mount);
        let fs = if fstype.is_empty() {
            sniff_fs(&path)
        } else {
            fstype
        };
        out.push(DeviceInfo {
            name: if model.is_empty() {
                name.clone()
            } else {
                format!("{name} — {model}")
            },
            path: path.clone(),
            size,
            kind: DeviceKind::Disk,
            fs,
            label: String::new(),
            mount,
            removable,
            parent: None,
            free,
        });
        let mut parts: Vec<String> = fs::read_dir(&sys)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().join("partition").exists())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        parts.sort_by_key(|p| (p.len(), p.clone()));
        for p in parts {
            let psize = read(format!("{sys}/{p}/size")).parse::<u64>().unwrap_or(0) * 512;
            let ppath = format!("/dev/{p}");
            let (mount, fstype) = mounts.get(&ppath).cloned().unwrap_or_default();
            let free = free_space(&mount);
            let fs = if fstype.is_empty() {
                sniff_fs(&ppath)
            } else {
                fstype
            };
            out.push(DeviceInfo {
                name: p.clone(),
                path: ppath,
                size: psize,
                kind: DeviceKind::Partition,
                fs,
                label: String::new(),
                mount,
                removable,
                parent: Some(path.clone()),
                free,
            });
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_list() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/dev") else {
        return out;
    };
    let mut names: Vec<String> = dir
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.strip_prefix("disk")
                .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit() || c == 's'))
        })
        .collect();
    names.sort();
    for n in names {
        let path = format!("/dev/r{n}");
        let size = Device::open(&path).map(|d| d.size()).unwrap_or(0);
        let is_part = n[4..].contains('s');
        out.push(DeviceInfo {
            name: n.clone(),
            path,
            size,
            kind: if is_part {
                DeviceKind::Partition
            } else {
                DeviceKind::Disk
            },
            fs: String::new(),
            label: String::new(),
            mount: String::new(),
            removable: false,
            parent: is_part.then(|| format!("/dev/r{}", &n[..n[4..].find('s').unwrap() + 4])),
            free: None,
        });
    }
    out
}

#[cfg(windows)]
fn windows_list() -> Vec<DeviceInfo> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    // Opening with zero access rights is allowed without elevation and is
    // enough for the query IOCTLs below.
    let query_open = |p: &str| OpenOptions::new().access_mode(0).share_mode(0x7).open(p);
    let mut out = Vec::new();
    for i in 0..32 {
        let path = format!(r"\\.\PhysicalDrive{i}");
        let Ok(f) = query_open(&path) else { continue };
        let h = f.as_raw_handle() as win::Handle;
        let mut geo = [0u8; 256];
        let size = win::ioctl(h, win::IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], &mut geo)
            .map(|_| u64::from_le_bytes(geo[24..32].try_into().unwrap()))
            .unwrap_or(0);
        let q = [0u8; 12];
        let mut desc = [0u8; 1024];
        let (mut model, mut removable) = (String::new(), false);
        if win::ioctl(h, win::IOCTL_STORAGE_QUERY_PROPERTY, &q[..], &mut desc).is_some() {
            removable = desc[10] != 0;
            let bus = u32::from_le_bytes(desc[28..32].try_into().unwrap());
            removable |= bus == 7 || bus == 12 || bus == 13; // USB, SD, MMC
            let grab = |at: usize| {
                let off = u32::from_le_bytes(desc[at..at + 4].try_into().unwrap()) as usize;
                if off == 0 || off >= desc.len() {
                    return String::new();
                }
                let end = desc[off..]
                    .iter()
                    .position(|&b| b == 0)
                    .map_or(desc.len(), |e| off + e);
                String::from_utf8_lossy(&desc[off..end]).trim().to_string()
            };
            model = format!("{} {}", grab(12), grab(16)).trim().to_string();
        }
        out.push(DeviceInfo {
            name: if model.is_empty() {
                format!("Disk {i}")
            } else {
                format!("Disk {i} — {model}")
            },
            path,
            size,
            kind: DeviceKind::Disk,
            fs: String::new(),
            label: String::new(),
            mount: String::new(),
            removable,
            parent: None,
            free: None,
        });
    }
    // SAFETY: trivial Win32 calls with valid, NUL-terminated buffers.
    let mask = unsafe { win::GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = win::wide(&format!("{letter}:\\"));
        let dtype = unsafe { win::GetDriveTypeW(root.as_ptr()) };
        if !(2..=3).contains(&dtype) && dtype != 6 {
            continue; // skip network shares and optical drives
        }
        let mut label = [0u16; 261];
        let mut fsname = [0u16; 261];
        let (mut serial, mut maxc, mut flags) = (0u32, 0u32, 0u32);
        unsafe {
            win::GetVolumeInformationW(
                root.as_ptr(),
                label.as_mut_ptr(),
                261,
                &mut serial,
                &mut maxc,
                &mut flags,
                fsname.as_mut_ptr(),
                261,
            );
        }
        let (mut free, mut total, mut tfree) = (0u64, 0u64, 0u64);
        unsafe { win::GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, &mut tfree) };
        let path = format!(r"\\.\{letter}:");
        let mut parent = None;
        if let Ok(f) = query_open(&path) {
            let mut ext = [0u8; 256];
            if win::ioctl(
                f.as_raw_handle() as win::Handle,
                win::IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
                &[],
                &mut ext,
            )
            .is_some()
            {
                let n = u32::from_le_bytes(ext[8..12].try_into().unwrap());
                parent = Some(format!(r"\\.\PhysicalDrive{n}"));
            }
        }
        let label = win::from_wide(&label);
        out.push(DeviceInfo {
            name: if label.is_empty() {
                format!("{letter}:")
            } else {
                format!("{letter}: [{label}]")
            },
            path,
            size: total,
            kind: DeviceKind::Partition,
            fs: win::from_wide(&fsname),
            label,
            mount: format!("{letter}:\\"),
            removable: dtype == 2,
            parent,
            free: (total > 0).then_some(tfree),
        });
    }
    out
}

/// Where a folder lives: the device holding it and the folder's path inside
/// that volume ('/'-separated, no leading slash) — so "scan my Desktop"
/// becomes "scan C: and show what was under Users/Name/Desktop".
pub fn locate_folder(path: &std::path::Path) -> Option<(DeviceInfo, String)> {
    let full = std::fs::canonicalize(path).ok()?;
    let mut s = full.to_string_lossy().replace('\\', "/");
    if let Some(r) = s.strip_prefix("//?/") {
        s = r.to_string();
    }
    let under = |m: &str| -> bool {
        let m = m.trim_end_matches('/');
        let head = s.get(..m.len()).unwrap_or("");
        let same = if cfg!(windows) {
            head.eq_ignore_ascii_case(m)
        } else {
            head == m
        };
        same && (s.len() == m.len() || s.as_bytes().get(m.len()) == Some(&b'/'))
    };
    let best = list_devices()
        .into_iter()
        .filter(|d| !d.mount.is_empty() && under(&d.mount.replace('\\', "/")))
        .max_by_key(|d| d.mount.len())?;
    let m = best.mount.replace('\\', "/");
    let rel = s[m.trim_end_matches('/').len()..]
        .trim_matches('/')
        .to_string();
    Some((best, rel))
}

/// Best-effort check whether writing into `out_dir` would land on the same
/// physical disk that `source` (a device path) lives on — which could
/// overwrite the very data being recovered.
pub fn same_disk(source: &str, out_dir: &std::path::Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        let disk_of = |devno: u64| -> Option<String> {
            let (maj, min) = (
                libc::major(devno as libc::dev_t),
                libc::minor(devno as libc::dev_t),
            );
            let link = std::fs::canonicalize(format!("/sys/dev/block/{maj}:{min}")).ok()?;
            let link = if link.join("partition").exists() {
                link.parent()?.to_path_buf()
            } else {
                link
            };
            Some(link.file_name()?.to_string_lossy().into_owned())
        };
        let Ok(src) = std::fs::metadata(source) else {
            return false;
        };
        if src.file_type().is_file() {
            return false; // an image file is not deleted space
        }
        let src_disk = disk_of(src.rdev());
        let mut dir = out_dir.to_path_buf();
        while !dir.exists() {
            if !dir.pop() {
                return false;
            }
        }
        let Ok(out) = std::fs::metadata(&dir) else {
            return false;
        };
        return src_disk.is_some() && src_disk == disk_of(out.dev());
    }
    #[cfg(windows)]
    {
        let out = out_dir.to_string_lossy().to_ascii_uppercase();
        let src = source.to_ascii_uppercase();
        let out_letter = out.chars().next().filter(|c| c.is_ascii_alphabetic());
        if let Some(l) = out_letter {
            if src == format!(r"\\.\{l}:") {
                return true;
            }
            if src.starts_with(r"\\.\PHYSICALDRIVE") {
                let parent = list_devices()
                    .into_iter()
                    .find(|d| d.mount.to_ascii_uppercase().starts_with(l))
                    .and_then(|d| d.parent);
                return parent.is_some_and(|p| p.to_ascii_uppercase() == src);
            }
            // Image file on the same drive is harmless (it is not deleted space).
        }
        return false;
    }
    #[allow(unreachable_code)]
    {
        let _ = (source, out_dir);
        false
    }
}

//! Writing recovered files out. The source is only ever read.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::device::{read_resilient, AlignedBuf, ReadAt};
use crate::fs::sanitize_name;
use crate::types::{FileData, FoundFile, Origin};
use crate::volume::Volume;

#[derive(Default)]
pub struct RecoverProgress {
    pub files_done: AtomicUsize,
    pub files_total: AtomicUsize,
    pub bytes_done: AtomicU64,
    pub bytes_total: AtomicU64,
    pub failed: AtomicUsize,
    pub cancel: AtomicBool,
}

#[derive(Debug, Default)]
pub struct RecoverReport {
    pub written: usize,
    pub bytes: u64,
    pub failed: Vec<(String, String)>,
    pub bad_sectors: u64,
    pub out_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RecoverOptions {
    /// Recreate the original folder structure for filesystem results.
    pub keep_folders: bool,
    pub threads: usize,
}

impl Default for RecoverOptions {
    fn default() -> Self {
        RecoverOptions {
            keep_folders: true,
            threads: 4,
        }
    }
}

fn target_dir(out: &Path, f: &FoundFile, vols: &[Volume], keep: bool) -> PathBuf {
    let mut p = out.to_path_buf();
    if f.origin == Origin::Carved {
        p.push("Carved");
        p.push(if f.ext.is_empty() {
            "unknown".to_string()
        } else {
            f.ext.to_ascii_uppercase()
        });
        return p;
    }
    let vol = vols
        .iter()
        .find(|v| v.index == f.volume)
        .map_or_else(|| "Volume".to_string(), |v| v.desc.clone());
    p.push(sanitize_name(&vol));
    if keep {
        for comp in f.dir.split('/').filter(|c| !c.is_empty()) {
            p.push(sanitize_name(comp));
        }
    }
    p
}

/// Create `dir/name`, adding " (n)" before the extension if it exists.
fn create_unique(dir: &Path, name: &str) -> io::Result<(File, PathBuf)> {
    let name = sanitize_name(name);
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    for n in 0..10_000 {
        let candidate = if n == 0 {
            dir.join(&name)
        } else {
            dir.join(format!("{stem} ({n}){ext}"))
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => return Ok((f, candidate)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many name collisions"))
}

/// Stream a found file's content to `w`. Returns bad-sector count.
pub fn copy_content(
    src: &dyn ReadAt,
    f: &FoundFile,
    out: &mut File,
    buf: &mut AlignedBuf,
    progress: Option<&RecoverProgress>,
) -> io::Result<u64> {
    let mut bad = 0;
    match &f.data {
        FileData::Resident(d) => {
            let n = (d.len() as u64).min(f.size) as usize;
            out.write_all(&d[..n])?;
        }
        FileData::Extents(exts) => {
            let mut left = f.size;
            for e in exts {
                if left == 0 {
                    break;
                }
                let len = e.len.min(left);
                if e.sparse {
                    out.seek(SeekFrom::Current(len as i64))?;
                } else {
                    let mut done = 0u64;
                    while done < len {
                        if progress.is_some_and(|p| p.cancel.load(Relaxed)) {
                            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                        }
                        let n = ((len - done) as usize).min(buf.len());
                        let (got, b) = read_resilient(src, e.offset + done, &mut buf[..n]);
                        bad += b;
                        if got == 0 {
                            break;
                        }
                        out.write_all(&buf[..got])?;
                        done += got as u64;
                        if let Some(p) = progress {
                            p.bytes_done.fetch_add(got as u64, Relaxed);
                        }
                    }
                }
                left -= len;
            }
            // Sparse tail / short device: make the length exact.
            out.set_len(f.size - left)?;
        }
    }
    Ok(bad)
}

/// Recover `files` into `out_dir`. Files are processed in on-disk order so
/// the source is read as sequentially as possible.
pub fn recover(
    src: &dyn ReadAt,
    files: &[FoundFile],
    vols: &[Volume],
    out_dir: &Path,
    opts: &RecoverOptions,
    progress: &RecoverProgress,
) -> RecoverReport {
    let mut order: Vec<&FoundFile> = files.iter().collect();
    order.sort_by_key(|f| f.first_offset());
    progress.files_total.store(order.len(), Relaxed);
    progress
        .bytes_total
        .store(order.iter().map(|f| f.size).sum(), Relaxed);

    let next = AtomicUsize::new(0);
    let report = Mutex::new(RecoverReport {
        out_dir: out_dir.to_path_buf(),
        ..Default::default()
    });
    let dirs_made: Mutex<HashMap<PathBuf, ()>> = Mutex::new(HashMap::new());
    let threads = opts.threads.clamp(1, 8).min(order.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut buf = AlignedBuf::new(4 << 20);
                loop {
                    if progress.cancel.load(Relaxed) {
                        break;
                    }
                    let i = next.fetch_add(1, Relaxed);
                    let Some(f) = order.get(i) else { break };
                    let dir = target_dir(out_dir, f, vols, opts.keep_folders);
                    let result = (|| -> io::Result<(u64, u64)> {
                        if !dirs_made.lock().unwrap().contains_key(&dir) {
                            fs::create_dir_all(&dir)?;
                            dirs_made.lock().unwrap().insert(dir.clone(), ());
                        }
                        let (mut file, path) = create_unique(&dir, &f.name)?;
                        let bad = match copy_content(src, f, &mut file, &mut buf, Some(progress)) {
                            Ok(b) => b,
                            Err(e) => {
                                drop(file);
                                let _ = fs::remove_file(&path);
                                return Err(e);
                            }
                        };
                        // `modified` is local wall-clock time; the OS wants UTC.
                        let utc = f.modified.map(|t| t - crate::bytes::local_offset());
                        if let Some(t) = utc.filter(|&t| t > 0) {
                            let _ = file.set_modified(
                                SystemTime::UNIX_EPOCH + Duration::from_secs(t as u64),
                            );
                        }
                        Ok((f.size, bad))
                    })();
                    let mut r = report.lock().unwrap();
                    match result {
                        Ok((n, bad)) => {
                            r.written += 1;
                            r.bytes += n;
                            r.bad_sectors += bad;
                        }
                        Err(e) => {
                            progress.failed.fetch_add(1, Relaxed);
                            r.failed.push((f.full_path(), e.to_string()));
                        }
                    }
                    progress.files_done.fetch_add(1, Relaxed);
                }
            });
        }
    });
    report.into_inner().unwrap()
}

/// First `max` bytes of a found file (for previews).
pub fn read_head(src: &dyn ReadAt, f: &FoundFile, max: usize) -> Vec<u8> {
    let want = (f.size as usize).min(max);
    let mut out = Vec::with_capacity(want);
    match &f.data {
        FileData::Resident(d) => out.extend_from_slice(&d[..d.len().min(want)]),
        FileData::Extents(exts) => {
            for e in exts {
                if out.len() >= want {
                    break;
                }
                let n = (e.len as usize).min(want - out.len());
                let start = out.len();
                out.resize(start + n, 0);
                if !e.sparse {
                    let (got, _) = read_resilient(src, e.offset, &mut out[start..]);
                    out.truncate(start + got);
                    if got < n {
                        break;
                    }
                }
            }
        }
    }
    out
}

//! Scan orchestration: partitions → filesystem metadata → signature carving,
//! with lock-free progress counters and a result event stream for the UI.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::carve::{carve, CarveOptions};
use crate::device::ReadAt;
use crate::fs::{self, merge_ranges, FsReport};
use crate::types::{Condition, FileData, FoundFile};
use crate::volume::{find_volumes, FsType, Volume};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Idle = 0,
    Analyzing,
    Filesystem,
    Carving,
    Done,
    Cancelled,
}

impl Phase {
    fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::Analyzing,
            2 => Phase::Filesystem,
            3 => Phase::Carving,
            4 => Phase::Done,
            5 => Phase::Cancelled,
            _ => Phase::Idle,
        }
    }
}

#[derive(Default)]
pub struct Progress {
    phase: AtomicU8,
    done: AtomicU64,
    total: AtomicU64,
    found: AtomicU64,
    bad: AtomicU64,
    cancel: AtomicBool,
    ids: AtomicU64,
    status: Mutex<String>,
    started: Mutex<Option<Instant>>,
    phase_started: Mutex<Option<Instant>>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub phase: Phase,
    pub done: u64,
    pub total: u64,
    pub found: u64,
    pub bad_sectors: u64,
    pub status: String,
    pub elapsed: Duration,
    pub phase_elapsed: Duration,
}

impl Snapshot {
    /// Bytes per second in the current phase.
    pub fn rate(&self) -> f64 {
        let s = self.phase_elapsed.as_secs_f64();
        if s > 0.2 {
            self.done as f64 / s
        } else {
            0.0
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let r = self.rate();
        (r > 0.0 && self.total > self.done)
            .then(|| Duration::from_secs_f64((self.total - self.done) as f64 / r))
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f64 / self.total as f64).min(1.0) as f32
        }
    }
}

impl Progress {
    pub fn snapshot(&self) -> Snapshot {
        let el =
            |m: &Mutex<Option<Instant>>| m.lock().unwrap().map_or(Duration::ZERO, |t| t.elapsed());
        Snapshot {
            phase: Phase::from_u8(self.phase.load(Relaxed)),
            done: self.done.load(Relaxed),
            total: self.total.load(Relaxed),
            found: self.found.load(Relaxed),
            bad_sectors: self.bad.load(Relaxed),
            status: self.status.lock().unwrap().clone(),
            elapsed: el(&self.started),
            phase_elapsed: el(&self.phase_started),
        }
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Relaxed)
    }
    pub(crate) fn add_done(&self, n: u64) {
        self.done.fetch_add(n, Relaxed);
    }
    pub(crate) fn add_total(&self, n: u64) {
        self.total.fetch_add(n, Relaxed);
    }
    pub(crate) fn add_bad(&self, n: u64) {
        if n > 0 {
            self.bad.fetch_add(n, Relaxed);
        }
    }
    fn begin(&self, p: Phase) {
        self.phase.store(p as u8, Relaxed);
        self.done.store(0, Relaxed);
        self.total.store(0, Relaxed);
        *self.phase_started.lock().unwrap() = Some(Instant::now());
    }
}

pub enum ScanEvent {
    Volumes(Vec<Volume>),
    /// New results (filesystem or provisional carved).
    Found(Vec<FoundFile>),
    /// Final carved set: replaces every provisional carved result.
    CarveFinal(Vec<FoundFile>),
    Log(String),
    Done(ScanSummary),
}

#[derive(Clone, Debug, Default)]
pub struct ScanSummary {
    pub volumes: Vec<Volume>,
    pub fs_found: usize,
    pub carved: usize,
    pub bytes_carved: u64,
    pub elapsed: Duration,
    pub bad_sectors: u64,
    pub cancelled: bool,
}

/// What filesystem scanners and the carver get to talk to the outside.
pub struct ScanCtx<'a> {
    pub progress: &'a Progress,
    pub tx: &'a Sender<ScanEvent>,
    /// List deleted files (quick scan). When false, scanners only report
    /// free space for the deep scan.
    pub list_files: bool,
    pub(crate) covered: Mutex<Vec<(u64, u64)>>,
}

impl ScanCtx<'_> {
    pub fn cancelled(&self) -> bool {
        self.progress.is_cancelled()
    }
    pub fn next_id(&self) -> u64 {
        self.progress.ids.fetch_add(1, Relaxed)
    }
    pub fn set_status(&self, s: String) {
        *self.progress.status.lock().unwrap() = s;
    }
    pub fn log(&self, s: String) {
        let _ = self.tx.send(ScanEvent::Log(s));
    }
}

/// Buffers results and ships them to the UI in batches.
pub struct Batch<'a> {
    ctx: &'a ScanCtx<'a>,
    buf: Vec<FoundFile>,
}

impl<'a> Batch<'a> {
    pub fn new(ctx: &'a ScanCtx<'a>) -> Self {
        Batch {
            ctx,
            buf: Vec::new(),
        }
    }
    pub fn push(&mut self, f: FoundFile) {
        self.buf.push(f);
        if self.buf.len() >= 512 {
            self.flush();
        }
    }
    pub fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let v = std::mem::take(&mut self.buf);
        self.ctx.progress.found.fetch_add(v.len() as u64, Relaxed);
        if self.ctx.list_files {
            // Remember intact filesystem results so the carver skips them.
            let mut cov = self.ctx.covered.lock().unwrap();
            for f in v.iter().filter(|f| {
                f.condition == Condition::Good && f.origin != crate::types::Origin::Carved
            }) {
                if let FileData::Extents(e) = &f.data {
                    cov.extend(e.iter().filter(|x| !x.sparse).map(|x| (x.offset, x.len)));
                }
            }
        }
        let _ = self.ctx.tx.send(ScanEvent::Found(v));
    }
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Read filesystem metadata (names, folders, dates).
    pub quick: bool,
    /// Signature carving.
    pub deep: bool,
    /// Carve only unallocated space when the filesystem is understood.
    pub free_only: bool,
    /// Test every byte offset instead of every sector start (much slower).
    pub byte_level: bool,
    /// `Category::bit()` mask of types to carve.
    pub categories: u8,
    pub threads: usize,
    pub chunk_mb: usize,
    /// Deep scan only these (canonical) extensions — see
    /// [`crate::Target::wanted_exts`]. `None` = every supported type.
    pub only_exts: Option<Vec<String>>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            quick: true,
            deep: true,
            free_only: true,
            byte_level: false,
            categories: 0x3F,
            threads: std::thread::available_parallelism()
                .map_or(4, |n| n.get())
                .min(8),
            chunk_mb: 16,
            only_exts: None,
        }
    }
}

pub struct ScanHandle {
    pub progress: Arc<Progress>,
    pub events: Receiver<ScanEvent>,
    join: Option<JoinHandle<()>>,
}

impl ScanHandle {
    pub fn cancel(&self) {
        self.progress.cancel();
    }
    pub fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(|j| j.is_finished())
    }
    pub fn wait(mut self) {
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn start_scan(src: Arc<dyn ReadAt>, opts: ScanOptions) -> ScanHandle {
    let progress = Arc::new(Progress::default());
    let (tx, rx) = unbounded();
    let p = progress.clone();
    let join = std::thread::Builder::new()
        .name("pulih-scan".into())
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&*src, &opts, &p, &tx)
            }));
            if r.is_err() {
                let _ = tx.send(ScanEvent::Log("internal error: scan aborted".into()));
                p.phase.store(Phase::Done as u8, Relaxed);
                let _ = tx.send(ScanEvent::Done(ScanSummary::default()));
            }
        })
        .expect("spawn scan thread");
    ScanHandle {
        progress,
        events: rx,
        join: Some(join),
    }
}

fn scan_fs(src: &dyn ReadAt, v: &Volume, ctx: &ScanCtx) -> std::io::Result<FsReport> {
    match v.fs {
        FsType::Ntfs => fs::ntfs::scan(src, v, ctx),
        FsType::Fat12 | FsType::Fat16 | FsType::Fat32 => fs::fat::scan(src, v, ctx),
        FsType::ExFat => fs::exfat::scan(src, v, ctx),
        _ => Ok(FsReport::default()),
    }
}

fn run(src: &dyn ReadAt, opts: &ScanOptions, progress: &Progress, tx: &Sender<ScanEvent>) {
    *progress.started.lock().unwrap() = Some(Instant::now());
    progress.begin(Phase::Analyzing);
    let size = src.size();
    let vols = find_volumes(src);
    let _ = tx.send(ScanEvent::Volumes(vols.clone()));
    let ctx = ScanCtx {
        progress,
        tx,
        list_files: opts.quick,
        covered: Mutex::new(Vec::new()),
    };
    let mut summary = ScanSummary {
        volumes: vols.clone(),
        ..Default::default()
    };

    // Free space per volume (None = unknown → carve the whole volume).
    let mut free: Vec<Option<Vec<(u64, u64)>>> = vec![None; vols.len()];
    if opts.quick || (opts.deep && opts.free_only) {
        progress.begin(Phase::Filesystem);
        for (i, v) in vols.iter().enumerate() {
            if ctx.cancelled() {
                break;
            }
            if !v.fs.supported() {
                continue;
            }
            ctx.set_status(format!("{} — {}", v.desc, v.fs.name()));
            match scan_fs(src, v, &ctx) {
                Ok(rep) => {
                    summary.fs_found += rep.found;
                    free[i] = rep.free;
                }
                Err(e) => ctx.log(format!("{}: {e}", v.desc)),
            }
        }
    }

    if opts.deep && !ctx.cancelled() {
        progress.begin(Phase::Carving);
        let mut ranges: Vec<(u64, u64)> = Vec::new();
        if opts.free_only {
            // Space outside any partition (deleted partitions live there).
            let mut cursor = 0u64;
            let mut sorted: Vec<&Volume> = vols.iter().collect();
            sorted.sort_by_key(|v| v.offset);
            for v in &sorted {
                if v.offset > cursor {
                    ranges.push((cursor, v.offset - cursor));
                }
                cursor = cursor.max(v.offset + v.size);
            }
            if cursor < size {
                ranges.push((cursor, size - cursor));
            }
            for (i, v) in vols.iter().enumerate() {
                match &free[i] {
                    Some(f) => ranges.extend(f.iter().copied()),
                    None => ranges.push((v.offset, v.size)),
                }
            }
        } else {
            ranges.push((0, size));
        }
        ranges.retain(|r| r.1 > 0);
        merge_ranges(&mut ranges);
        let free_bytes: u64 = ranges.iter().map(|r| r.1).sum();
        ctx.set_status(format!(
            "Signature scan of {} {}",
            crate::bytes::fmt_size(free_bytes),
            if opts.free_only {
                "(unallocated space)"
            } else {
                "(entire device)"
            }
        ));
        let covered = std::mem::take(&mut *ctx.covered.lock().unwrap());
        let copts = CarveOptions {
            byte_level: opts.byte_level,
            categories: opts.categories,
            threads: opts.threads,
            chunk: opts.chunk_mb << 20,
            only_exts: opts.only_exts.clone(),
        };
        let carved = carve(src, &ranges, &covered, &vols, &copts, &ctx);
        summary.carved = carved.len();
        summary.bytes_carved = free_bytes;
        progress
            .found
            .store((summary.fs_found + carved.len()) as u64, Relaxed);
        let _ = tx.send(ScanEvent::CarveFinal(carved));
    }

    let cancelled = ctx.cancelled();
    summary.cancelled = cancelled;
    summary.elapsed = progress.snapshot().elapsed;
    summary.bad_sectors = progress.bad.load(Relaxed);
    progress.phase.store(
        if cancelled {
            Phase::Cancelled
        } else {
            Phase::Done
        } as u8,
        Relaxed,
    );
    let _ = tx.send(ScanEvent::Done(summary));
}

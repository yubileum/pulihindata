//! pulih-cli — headless front end (scripting, servers, benchmarks).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pulih_core::bytes::{fmt_size, fmt_time};
use pulih_core::{
    list_devices, recover, start_scan, Category, Condition, Device, FoundFile, Origin, Phase,
    ReadAt, RecoverOptions, RecoverProgress, ScanEvent, ScanOptions,
};

const USAGE: &str = "\
pulih-cli — recover deleted files

USAGE:
  pulih-cli list
  pulih-cli scan <device-or-image> [options]

SCAN OPTIONS:
  --quick-only        only read filesystem metadata (NTFS/FAT/exFAT)
  --deep-only         only signature carving
  --all-space         carve the entire device, not just unallocated space
  --byte-level        test every byte offset (slow; finds embedded files)
  --types LIST        comma list: image,video,audio,document,archive,other
  --threads N         carving worker threads (default: CPU count, max 8)
  --good-only         keep only results whose data looks intact
  --min-size BYTES    ignore smaller results
  --list              print every result
  --recover DIR       write the results into DIR (never the scanned disk!)
";

fn parse_types(s: &str) -> Option<u8> {
    let mut m = 0u8;
    for t in s.split(',') {
        m |= match t.trim().to_ascii_lowercase().as_str() {
            "image" | "images" | "photo" => Category::Image.bit(),
            "video" | "videos" => Category::Video.bit(),
            "audio" | "music" => Category::Audio.bit(),
            "document" | "documents" | "doc" => Category::Document.bit(),
            "archive" | "archives" => Category::Archive.bit(),
            "other" => Category::Other.bit(),
            "all" => 0x3F,
            _ => return None,
        };
    }
    Some(m)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => {
            for d in list_devices() {
                println!(
                    "{:<28} {:>10}  {:<8} {:<10} {}{}",
                    d.path,
                    fmt_size(d.size),
                    format!("{:?}", d.kind),
                    d.fs,
                    d.name,
                    if d.mount.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", d.mount)
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Some("scan") if args.len() >= 2 => scan(&args[1], &args[2..]),
        _ => {
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn scan(source: &str, rest: &[String]) -> ExitCode {
    let mut opts = ScanOptions::default();
    let (mut list, mut good_only, mut min_size) = (false, false, 0u64);
    let mut out: Option<PathBuf> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--quick-only" => opts.deep = false,
            "--deep-only" => opts.quick = false,
            "--all-space" => opts.free_only = false,
            "--byte-level" => opts.byte_level = true,
            "--list" => list = true,
            "--good-only" => good_only = true,
            "--types" => match it.next().and_then(|s| parse_types(s)) {
                Some(m) => opts.categories = m,
                None => return bad("--types expects e.g. image,video"),
            },
            "--threads" => match it.next().and_then(|s| s.parse().ok()) {
                Some(n) => opts.threads = n,
                None => return bad("--threads expects a number"),
            },
            "--min-size" => match it.next().and_then(|s| s.parse().ok()) {
                Some(n) => min_size = n,
                None => return bad("--min-size expects bytes"),
            },
            "--recover" => match it.next() {
                Some(d) => out = Some(PathBuf::from(d)),
                None => return bad("--recover expects a directory"),
            },
            other => return bad(&format!("unknown option {other}")),
        }
    }
    let dev = match Device::open(source) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("cannot open {source}: {e}");
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!("hint: raw disk access needs root / Administrator");
            }
            return ExitCode::FAILURE;
        }
    };
    if let Some(o) = &out {
        if pulih_core::same_disk(source, o) {
            eprintln!(
                "refusing to recover onto the disk being scanned ({}) — pick another drive",
                o.display()
            );
            return ExitCode::FAILURE;
        }
    }
    eprintln!("Scanning {} ({})", source, fmt_size(dev.size()));
    let handle = start_scan(dev.clone(), opts);
    let mut files: Vec<FoundFile> = Vec::new();
    let mut vols = Vec::new();
    let mut last = Instant::now();
    let summary = loop {
        match handle.events.recv_timeout(Duration::from_millis(250)) {
            Ok(ScanEvent::Volumes(v)) => {
                for x in &v {
                    eprintln!("  {} at {} — {}", x.desc, x.offset, fmt_size(x.size));
                }
                vols = v;
            }
            Ok(ScanEvent::Found(v)) => files.extend(v),
            Ok(ScanEvent::CarveFinal(v)) => {
                files.retain(|f| f.origin != Origin::Carved);
                files.extend(v);
            }
            Ok(ScanEvent::Log(s)) => eprintln!("\r  note: {s}"),
            Ok(ScanEvent::Done(s)) => break s,
            Err(_) => {}
        }
        if last.elapsed() > Duration::from_millis(500) {
            last = Instant::now();
            let s = handle.progress.snapshot();
            let phase = match s.phase {
                Phase::Filesystem => "metadata",
                Phase::Carving => "deep scan",
                _ => "analysing",
            };
            eprint!(
                "\r  {phase}: {:5.1}%  {}/s  found {}  ETA {}        ",
                s.fraction() * 100.0,
                fmt_size(s.rate() as u64),
                s.found,
                s.eta().map_or("-".into(), |d| format!("{}s", d.as_secs()))
            );
            let _ = std::io::stderr().flush();
        }
    };
    handle.wait();
    files.retain(|f| f.size >= min_size && (!good_only || f.condition == Condition::Good));
    eprintln!(
        "\r  done in {:.2}s: {} from filesystem metadata, {} by signature, {} bad sectors{}",
        summary.elapsed.as_secs_f64(),
        summary.fs_found,
        summary.carved,
        summary.bad_sectors,
        if summary.cancelled {
            " (cancelled)"
        } else {
            ""
        }
    );
    if summary.bytes_carved > 0 {
        eprintln!(
            "  deep scan throughput: {}/s over {}",
            fmt_size(
                (summary.bytes_carved as f64 / summary.elapsed.as_secs_f64().max(1e-6)) as u64
            ),
            fmt_size(summary.bytes_carved)
        );
    }
    if list {
        for f in &files {
            println!(
                "{:<7} {:<11} {:>10}  {:<16}  {:<45} {}",
                f.origin.label(),
                format!("{:?}", f.condition),
                fmt_size(f.size),
                f.modified.map_or("-".into(), fmt_time),
                f.full_path(),
                f.first_offset()
            );
        }
    }
    let mut by_cat = [0usize; 6];
    for f in &files {
        by_cat[f.category as usize] += 1;
    }
    eprintln!(
        "  {} files: {} images, {} videos, {} audio, {} documents, {} archives, {} other",
        files.len(),
        by_cat[0],
        by_cat[1],
        by_cat[2],
        by_cat[3],
        by_cat[4],
        by_cat[5]
    );
    if let Some(dir) = out {
        let prog = RecoverProgress::default();
        let t = Instant::now();
        let r = recover(
            &*dev,
            &files,
            &vols,
            &dir,
            &RecoverOptions::default(),
            &prog,
        );
        eprintln!(
            "  recovered {} files ({}) into {} in {:.2}s; {} failed",
            r.written,
            fmt_size(r.bytes),
            dir.display(),
            t.elapsed().as_secs_f64(),
            r.failed.len()
        );
        for (n, e) in r.failed.iter().take(20) {
            eprintln!("    failed: {n}: {e}");
        }
    }
    ExitCode::SUCCESS
}

fn bad(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

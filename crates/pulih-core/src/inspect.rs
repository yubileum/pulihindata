//! Look inside a found file — whatever its origin — for the preview panel:
//! document title and text, date a photo was taken, song tags.

use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::carve::formats::{Ctx, Stopper};
use crate::carve::meta;
use crate::carve::window::Window;
use crate::carve::WINDOW_CACHE;
use crate::device::{AlignedBuf, ReadAt};
use crate::target::canon_ext;
use crate::types::{FileData, FoundFile};

#[derive(Clone, Debug, Default)]
pub struct Inspection {
    pub title: Option<String>,
    pub info: Option<String>,
    /// Local wall-clock time from the content (taken / recorded / saved).
    pub time: Option<i64>,
    /// Readable text of Office/ODF documents.
    pub text: Option<String>,
}

/// A found file's content as its own byte stream (follows fragments).
struct FileSource<'a> {
    src: &'a dyn ReadAt,
    f: &'a FoundFile,
}

impl ReadAt for FileSource<'_> {
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        if off >= self.f.size || buf.is_empty() {
            return Ok(0);
        }
        let want = (buf.len() as u64).min(self.f.size - off) as usize;
        match &self.f.data {
            FileData::Resident(d) => {
                let n = want.min(d.len().saturating_sub(off as usize));
                buf[..n].copy_from_slice(&d[off as usize..off as usize + n]);
                Ok(n)
            }
            FileData::Extents(exts) => {
                let mut base = 0u64;
                for e in exts {
                    if off < base + e.len {
                        let inner = off - base;
                        let n = want.min((e.len - inner) as usize);
                        if e.sparse {
                            buf[..n].fill(0);
                            return Ok(n);
                        }
                        return self.src.read_at(e.offset + inner, &mut buf[..n]);
                    }
                    base += e.len;
                }
                Ok(0)
            }
        }
    }

    fn size(&self) -> u64 {
        self.f.size
    }
}

pub fn inspect(src: &dyn ReadAt, f: &FoundFile, text_chars: usize) -> Inspection {
    let fs = FileSource { src, f };
    let stop = Stopper::new();
    let ext = canon_ext(&f.ext);
    let mut c = Ctx {
        w: Window::new(&fs, 0, &[], AlignedBuf::new(WINDOW_CACHE)),
        stop: &stop,
        limit: f.size,
    };
    let m =
        catch_unwind(AssertUnwindSafe(|| meta::describe(&mut c, &ext, f.size))).unwrap_or_default();
    let text = if text_chars > 0 {
        catch_unwind(AssertUnwindSafe(|| {
            meta::document_text(&mut c, &ext, f.size, text_chars)
        }))
        .ok()
        .flatten()
    } else {
        None
    };
    Inspection {
        title: m.title,
        info: m.info,
        time: m.time,
        text,
    }
}

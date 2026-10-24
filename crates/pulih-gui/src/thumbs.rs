//! Photo thumbnails for the grid view. Decoding happens on a small worker
//! pool, only for items currently on screen; JPEGs use the tiny preview the
//! camera embedded in EXIF when there is one, which is ~100× cheaper than
//! decoding the full photo.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, TextureHandle, TextureOptions};
use pulih_core::bytes::Bytes;
use pulih_core::{read_head, Category, Device, FoundFile};

const SIZE: u32 = 192;
const KEEP: usize = 600;
const DECODABLE: [&str; 9] = [
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "jfif",
];

type Req = (Arc<Device>, FoundFile, egui::Context);
/// `None` = skipped (scrolled away), `Some(None)` = not decodable.
type Done = (u64, Option<Option<(usize, usize, Vec<u8>)>>);

pub struct Thumbs {
    req: Sender<Req>,
    done: Receiver<Done>,
    visible: Arc<Mutex<HashSet<u64>>>,
    pending: HashSet<u64>,
    cache: HashMap<u64, Option<TextureHandle>>,
    order: VecDeque<u64>,
}

pub fn can_thumb(f: &FoundFile) -> bool {
    f.category == Category::Image && DECODABLE.contains(&f.ext.as_str()) && f.size <= 40 << 20
}

impl Default for Thumbs {
    fn default() -> Self {
        let (req, rx) = unbounded::<Req>();
        let (tx, done) = unbounded::<Done>();
        let visible: Arc<Mutex<HashSet<u64>>> = Arc::default();
        for _ in 0..3 {
            let (rx, tx, visible) = (rx.clone(), tx.clone(), visible.clone());
            std::thread::spawn(move || {
                while let Ok((src, f, ctx)) = rx.recv() {
                    // Scrolled past already? Don't spend time on it.
                    if !visible.lock().unwrap().contains(&f.id) {
                        let _ = tx.send((f.id, None));
                        continue;
                    }
                    let r = std::panic::catch_unwind(|| decode(&src, &f)).ok().flatten();
                    let _ = tx.send((f.id, Some(r)));
                    ctx.request_repaint();
                }
            });
        }
        Thumbs {
            req,
            done,
            visible,
            pending: HashSet::new(),
            cache: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl Thumbs {
    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
        self.pending.clear();
    }

    /// Call once per frame with the ids currently on screen.
    pub fn set_visible(&mut self, ids: impl IntoIterator<Item = u64>) {
        let mut v = self.visible.lock().unwrap();
        v.clear();
        v.extend(ids);
    }

    pub fn pump(&mut self, ctx: &egui::Context) {
        for (id, r) in self.done.try_iter() {
            self.pending.remove(&id);
            let Some(r) = r else {
                continue; // skipped: retried when it comes back on screen
            };
            let tex = r.map(|(w, h, rgba)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                ctx.load_texture(format!("th{id}"), img, TextureOptions::LINEAR)
            });
            self.cache.insert(id, tex);
            self.order.push_back(id);
            while self.order.len() > KEEP {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }

    /// `Some(Some(tex))` ready, `Some(None)` not decodable, `None` loading.
    pub fn get(
        &mut self,
        src: &Arc<Device>,
        f: &FoundFile,
        ctx: &egui::Context,
    ) -> Option<Option<TextureHandle>> {
        if let Some(t) = self.cache.get(&f.id) {
            return Some(t.clone());
        }
        if !can_thumb(f) {
            return Some(None);
        }
        if self.pending.insert(f.id) {
            let _ = self.req.send((src.clone(), f.clone(), ctx.clone()));
        }
        None
    }
}

/// Embedded EXIF preview of a JPEG (IFD1 JPEGInterchangeFormat).
fn exif_thumb(jpg: &[u8]) -> Option<&[u8]> {
    let mut pos = 2usize;
    for _ in 0..16 {
        if jpg.u8_at(pos)? != 0xFF {
            return None;
        }
        let m = jpg.u8_at(pos + 1)?;
        let len = jpg.u16be(pos + 2)? as usize;
        if m == 0xE1 && jpg.at(pos + 4, 6)? == b"Exif\0\0" {
            let tiff = jpg.at(pos + 10, len.checked_sub(8)?)?;
            let le = tiff.first()? == &b'I';
            let r16 = |o: usize| if le { tiff.u16le(o) } else { tiff.u16be(o) };
            let r32 = |o: usize| if le { tiff.u32le(o) } else { tiff.u32be(o) };
            let ifd0 = r32(4)? as usize;
            let n0 = r16(ifd0)? as usize;
            let ifd1 = r32(ifd0 + 2 + n0 * 12)? as usize;
            if ifd1 == 0 {
                return None;
            }
            let n1 = r16(ifd1)? as usize;
            let (mut off, mut size) = (0usize, 0usize);
            for k in 0..n1.min(64) {
                let e = ifd1 + 2 + k * 12;
                match r16(e)? {
                    0x0201 => off = r32(e + 8)? as usize,
                    0x0202 => size = r32(e + 8)? as usize,
                    _ => {}
                }
            }
            return tiff.at(off, size).filter(|t| t.starts_with(&[0xFF, 0xD8]));
        }
        if m == 0xDA {
            return None;
        }
        pos += 2 + len;
    }
    None
}

fn decode(src: &Device, f: &FoundFile) -> Option<(usize, usize, Vec<u8>)> {
    let is_jpeg = matches!(f.ext.as_str(), "jpg" | "jpeg" | "jfif");
    let head = read_head(src, f, if is_jpeg { 128 << 10 } else { f.size as usize });
    let img = if is_jpeg {
        exif_thumb(&head)
            .and_then(|t| image::load_from_memory(t).ok())
            .filter(|i| i.width() >= 120)
            .or_else(|| image::load_from_memory(&read_head(src, f, f.size as usize)).ok())?
    } else {
        image::load_from_memory(&head).ok()?
    };
    let t = img.thumbnail(SIZE, SIZE).to_rgba8();
    Some((t.width() as usize, t.height() as usize, t.into_raw()))
}

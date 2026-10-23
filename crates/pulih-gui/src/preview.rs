//! Background preview loading so decoding a 20 MP photo never stalls the UI.

use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use pulih_core::{inspect, read_head, Category, Device, FoundFile, Inspection};

pub enum Preview {
    Image {
        w: u32,
        h: u32,
        rgba: Vec<u8>,
        full: (u32, u32),
    },
    Text(String),
    Hex(String),
}

pub type Msg = (u64, Preview, Inspection);

pub struct Previewer {
    tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
}

impl Default for Previewer {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Previewer { tx, rx }
    }
}

const DECODABLE: [&str; 9] = [
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "jfif",
];

impl Previewer {
    pub fn request(&self, src: Arc<Device>, f: FoundFile, ctx: eframe::egui::Context) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // What the content says about itself (title, text, date taken).
            let ins = match f.category {
                Category::Other | Category::Archive => Inspection::default(),
                _ => inspect(&*src, &f, 3000),
            };
            let p = match &ins.text {
                Some(t) => Preview::Text(t.clone()),
                None => build(&src, &f),
            };
            let _ = tx.send((f.id, p, ins));
            ctx.request_repaint();
        });
    }
}

fn build(src: &Device, f: &FoundFile) -> Preview {
    if f.category == Category::Image && DECODABLE.contains(&f.ext.as_str()) && f.size <= 64 << 20 {
        let data = read_head(src, f, 64 << 20);
        if let Ok(img) = image::load_from_memory(&data) {
            let full = (img.width(), img.height());
            let t = img.thumbnail(640, 640).to_rgba8();
            return Preview::Image {
                w: t.width(),
                h: t.height(),
                rgba: t.into_raw(),
                full,
            };
        }
    }
    let head = read_head(src, f, 4096);
    let printable = head
        .iter()
        .filter(|&&b| b == b'\n' || b == b'\r' || b == b'\t' || (32..127).contains(&b))
        .count();
    if !head.is_empty() && printable * 100 / head.len() > 95 {
        return Preview::Text(String::from_utf8_lossy(&head[..head.len().min(2048)]).into_owned());
    }
    let mut s = String::new();
    for (i, row) in head.chunks(16).take(32).enumerate() {
        s.push_str(&format!("{:04x}  ", i * 16));
        for b in row {
            s.push_str(&format!("{b:02x} "));
        }
        for _ in row.len()..16 {
            s.push_str("   ");
        }
        s.push(' ');
        s.extend(row.iter().map(|&b| {
            if (32..127).contains(&b) {
                b as char
            } else {
                '·'
            }
        }));
        s.push('\n');
    }
    Preview::Hex(s)
}

//! "What are you looking for?" — the search form shared by the start screen
//! (before scanning) and the results panel (live filtering), plus the
//! original-folder tree.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use eframe::egui::{self, Sense, Stroke, Vec2};
use pulih_core::{canon_ext, parse_date, Category, FoundFile, Origin, Target, Volume};

use crate::i18n::T;
use crate::theme::{self, pal, semi};

/// Type chips: label → canonical extensions.
pub const TYPES: &[(Category, &str, &[&str])] = &[
    (Category::Image, "JPG", &["jpg"]),
    (Category::Image, "PNG", &["png"]),
    (Category::Image, "HEIC", &["heic", "heif", "avif"]),
    (Category::Image, "GIF", &["gif"]),
    (Category::Image, "BMP", &["bmp"]),
    (Category::Image, "TIFF", &["tif"]),
    (Category::Image, "WEBP", &["webp"]),
    (Category::Image, "PSD", &["psd", "psb"]),
    (
        Category::Image,
        "RAW",
        &[
            "cr2", "cr3", "nef", "nrw", "arw", "dng", "orf", "rw2", "raf", "pef", "srw",
        ],
    ),
    (Category::Video, "MP4", &["mp4", "m4v"]),
    (Category::Video, "MOV", &["mov"]),
    (Category::Video, "3GP", &["3gp", "3g2"]),
    (Category::Video, "MKV", &["mkv"]),
    (Category::Video, "WEBM", &["webm"]),
    (Category::Video, "AVI", &["avi"]),
    (Category::Video, "WMV", &["wmv", "asf"]),
    (Category::Video, "FLV", &["flv", "f4v"]),
    (Category::Video, "MTS", &["mts", "ts"]),
    (Category::Video, "MPG", &["mpg"]),
    (Category::Audio, "MP3", &["mp3"]),
    (Category::Audio, "M4A", &["m4a", "m4b", "aac"]),
    (Category::Audio, "WAV", &["wav"]),
    (Category::Audio, "FLAC", &["flac"]),
    (Category::Audio, "OGG", &["ogg", "opus", "oga", "spx"]),
    (Category::Audio, "WMA", &["wma"]),
    (Category::Audio, "AMR", &["amr"]),
    (Category::Audio, "AIFF", &["aif"]),
    (Category::Audio, "MIDI", &["mid"]),
    (Category::Document, "DOCX", &["docx", "docm", "dotx"]),
    (Category::Document, "DOC", &["doc", "dot"]),
    (Category::Document, "XLSX", &["xlsx", "xlsm"]),
    (Category::Document, "XLS", &["xls"]),
    (Category::Document, "PPTX", &["pptx", "ppsx"]),
    (Category::Document, "PPT", &["ppt", "pps"]),
    (Category::Document, "PDF", &["pdf"]),
    (Category::Document, "TXT", &["txt", "csv", "md"]),
    (Category::Document, "RTF", &["rtf"]),
    (Category::Document, "ODT", &["odt", "ods", "odp", "odg"]),
    (Category::Document, "EPUB", &["epub"]),
    (Category::Archive, "ZIP", &["zip"]),
    (Category::Archive, "RAR", &["rar"]),
    (Category::Archive, "7Z", &["7z"]),
    (Category::Archive, "APK", &["apk"]),
    (Category::Other, "EXE", &["exe", "dll", "msi"]),
    (Category::Other, "SQLite", &["sqlite"]),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DatePreset {
    Any,
    Week,
    Month,
    Year,
    Custom,
}

#[derive(Clone)]
pub struct Find {
    pub names: String,
    pub exts: BTreeSet<String>,
    pub folder: String,
    pub date: DatePreset,
    pub from: String,
    pub to: String,
    pub min_mb: f64,
    pub max_mb: f64,
    pub include_unnamed: bool,
}

impl Default for Find {
    fn default() -> Self {
        Find {
            names: String::new(),
            exts: BTreeSet::new(),
            folder: String::new(),
            date: DatePreset::Any,
            from: String::new(),
            to: String::new(),
            min_mb: 0.0,
            max_mb: 0.0,
            include_unnamed: true,
        }
    }
}

fn now_local() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    now + pulih_core::bytes::local_offset()
}

impl Find {
    pub fn target(&self) -> Target {
        let mut t = Target::default();
        t.set_names(&self.names);
        t.set_exts(self.exts.iter().map(String::as_str));
        t.folder = self.folder.trim().replace('\\', "/").to_lowercase();
        t.min_size = (self.min_mb * 1048576.0) as u64;
        t.max_size = (self.max_mb * 1048576.0) as u64;
        t.include_unnamed = self.include_unnamed;
        let day = 86_400;
        (t.after, t.before) = match self.date {
            DatePreset::Any => (None, None),
            DatePreset::Week => (Some(now_local() - 7 * day), None),
            DatePreset::Month => (Some(now_local() - 30 * day), None),
            DatePreset::Year => (Some(now_local() - 365 * day), None),
            DatePreset::Custom => (
                parse_date(&self.from),
                parse_date(&self.to).map(|d| d + day),
            ),
        };
        t
    }

    pub fn is_active(&self) -> bool {
        !self.target().is_empty()
    }
}

pub const DATE_PRESETS: [DatePreset; 5] = [
    DatePreset::Any,
    DatePreset::Week,
    DatePreset::Month,
    DatePreset::Year,
    DatePreset::Custom,
];

/// Toggle chip: soft blue when on, outlined when off.
pub fn chip(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let p = pal(ui);
    let (fg, bg, stroke) = if on {
        (p.primary, p.primary_soft, Stroke::new(1.0, p.primary))
    } else {
        (p.text, p.card, Stroke::new(1.0, p.border))
    };
    ui.add(
        egui::Button::new(semi(text).size(12.5).color(fg))
            .fill(bg)
            .stroke(stroke)
            .corner_radius(14)
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

/// Type chips grouped by category. With `counts`, only types that occur
/// (or are selected) are shown, each with its number of results.
pub fn type_picker(
    ui: &mut egui::Ui,
    exts: &mut BTreeSet<String>,
    counts: Option<&HashMap<String, usize>>,
    t: &T,
) -> bool {
    let mut changed = false;
    let count_of = |list: &[&str]| {
        counts.map(|m| {
            list.iter()
                .map(|e| m.get(*e).copied().unwrap_or(0))
                .sum::<usize>()
        })
    };
    let p = pal(ui);
    for cat in Category::ALL {
        let chips: Vec<&(Category, &str, &[&str])> = TYPES
            .iter()
            .filter(|c| c.0 == cat)
            .filter(|c| {
                count_of(c.2).is_none_or(|n| n > 0) || c.2.iter().any(|e| exts.contains(*e))
            })
            .collect();
        if chips.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(5.0, 5.0);
            let all_on = chips.iter().all(|c| c.2.iter().all(|e| exts.contains(*e)));
            let head = ui.add_sized(
                [96.0, 26.0],
                egui::Button::new(
                    semi(format!("{} {}", cat_icon(cat), t.cats[cat as usize]))
                        .size(12.5)
                        .color(if all_on {
                            p.primary
                        } else {
                            theme::type_color("", cat)
                        }),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(Stroke::NONE),
            );
            if head.clicked() {
                for c in &chips {
                    for e in c.2 {
                        if all_on {
                            exts.remove(*e);
                        } else {
                            exts.insert(e.to_string());
                        }
                    }
                }
                changed = true;
            }
            for (_, name, list) in &chips {
                let on = list.iter().all(|e| exts.contains(*e));
                let text = match count_of(list) {
                    Some(n) => format!("{name}  {n}"),
                    None => name.to_string(),
                };
                if chip(ui, on, &text).clicked() {
                    for e in *list {
                        if on {
                            exts.remove(*e);
                        } else {
                            exts.insert(e.to_string());
                        }
                    }
                    changed = true;
                }
            }
        });
    }
    changed
}

pub fn date_picker(ui: &mut egui::Ui, f: &mut Find, t: &T) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (pr, l) in DATE_PRESETS.iter().zip(t.date_presets) {
            if chip(ui, f.date == *pr, l).clicked() {
                f.date = *pr;
                changed = true;
            }
        }
    });
    if f.date == DatePreset::Custom {
        let bad = pal(ui).danger;
        ui.horizontal(|ui| {
            for (lbl, val) in [(t.find_from, &mut f.from), (t.find_to, &mut f.to)] {
                ui.label(lbl);
                let ok = val.trim().is_empty() || parse_date(val).is_some();
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(val)
                            .hint_text(t.date_hint)
                            .desired_width(96.0)
                            .text_color_opt((!ok).then_some(bad)),
                    )
                    .changed();
            }
        });
    }
    changed
}

pub fn size_picker(ui: &mut egui::Ui, f: &mut Find, t: &T) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(t.find_min);
        changed |= ui
            .add(
                egui::DragValue::new(&mut f.min_mb)
                    .speed(0.1)
                    .range(0.0..=1e6)
                    .suffix(" MB")
                    .max_decimals(1),
            )
            .changed();
        ui.label(t.find_max);
        changed |= ui
            .add(
                egui::DragValue::new(&mut f.max_mb)
                    .speed(0.1)
                    .range(0.0..=1e6)
                    .suffix(" MB")
                    .max_decimals(1),
            )
            .on_hover_text("0 = ∞")
            .changed();
    });
    changed
}

/// The full criteria form (start screen "More filters").
pub fn form(ui: &mut egui::Ui, f: &mut Find, t: &T) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let label = |ui: &mut egui::Ui, s: &str| {
        ui.add_space(6.0);
        ui.label(semi(s).color(p.muted).size(12.5));
    };
    label(ui, t.find_types);
    changed |= type_picker(ui, &mut f.exts, None, t);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(320.0);
            label(ui, t.find_folder);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut f.folder)
                        .hint_text(t.find_folder_hint)
                        .desired_width(300.0),
                )
                .changed();
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            label(ui, t.find_size);
            changed |= size_picker(ui, f, t);
        });
    });
    label(ui, t.find_date);
    changed |= date_picker(ui, f, t);
    ui.add_space(4.0);
    changed |= ui
        .checkbox(&mut f.include_unnamed, t.include_unnamed)
        .on_hover_text(t.include_unnamed_hint)
        .changed();
    changed
}

/// Short human summary of the active search ("skripsi · DOCX, PDF · 2024").
pub fn summary(f: &Find, t: &T) -> String {
    let mut parts = Vec::new();
    if !f.names.trim().is_empty() {
        parts.push(format!("\u{201c}{}\u{201d}", f.names.trim()));
    }
    if !f.exts.is_empty() {
        parts.push(type_summary(&f.exts));
    }
    if !f.folder.trim().is_empty() {
        parts.push(format!("{}: {}", t.find_folder, f.folder.trim()));
    }
    if f.date != DatePreset::Any {
        parts.push(date_summary(f, t));
    }
    if f.min_mb > 0.0 || f.max_mb > 0.0 {
        parts.push(size_summary(f));
    }
    parts.join("  ·  ")
}

pub fn type_summary(exts: &BTreeSet<String>) -> String {
    let labels: Vec<&str> = TYPES
        .iter()
        .filter(|c| c.2.iter().any(|e| exts.contains(*e)))
        .map(|c| c.1)
        .collect();
    labels.join(", ")
}

pub fn date_summary(f: &Find, t: &T) -> String {
    if f.date == DatePreset::Custom {
        format!("{} – {}", f.from.trim(), f.to.trim())
    } else {
        t.date_presets[DATE_PRESETS.iter().position(|p| *p == f.date).unwrap_or(0)].to_string()
    }
}

pub fn size_summary(f: &Find) -> String {
    format!(
        "{} – {} MB",
        f.min_mb,
        if f.max_mb > 0.0 {
            f.max_mb.to_string()
        } else {
            "∞".into()
        }
    )
}

/// Number of results per canonical extension.
pub fn ext_counts(files: &[FoundFile]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for f in files {
        *m.entry(canon_ext(&f.ext)).or_insert(0) += 1;
    }
    m
}

pub fn cat_icon(c: Category) -> &'static str {
    match c {
        Category::Image => "🖼",
        Category::Video => "🎞",
        Category::Audio => "🎵",
        Category::Document => "📄",
        Category::Archive => "📦",
        Category::Other => "⚙",
    }
}

/// Small leading icon of a navigation row.
#[derive(Clone, Copy)]
pub enum Mini<'a> {
    Glyph(&'a str),
    Folder,
    Volume,
    Search,
    Dot(egui::Color32),
}

/// Full-width navigation row: label on the left, count on the right.
pub fn nav_row(
    ui: &mut egui::Ui,
    icon: Mini,
    label: &str,
    count: Option<usize>,
    selected: bool,
) -> egui::Response {
    let p = pal(ui);
    let h = 28.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    let bg = if selected {
        Some(p.primary_soft)
    } else if resp.hovered() {
        Some(p.faint)
    } else {
        None
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, 6, bg);
    }
    let fg = if selected { p.primary } else { p.text };
    let font = |sz: f32| {
        egui::FontId::new(
            sz,
            egui::FontFamily::Name(if selected { "bold" } else { "semibold" }.into()),
        )
    };
    let mut x = rect.left() + 6.0;
    let ic = egui::Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
    let painter = ui.painter();
    match icon {
        Mini::Glyph(g) => {
            painter.text(
                ic.center(),
                egui::Align2::CENTER_CENTER,
                g,
                egui::FontId::proportional(13.0),
                fg,
            );
        }
        Mini::Folder => theme::paint_drive(painter, ic, theme::DriveIcon::Folder, p),
        Mini::Volume => theme::paint_drive(painter, ic.expand(1.0), theme::DriveIcon::Partition, p),
        Mini::Search => crate::home::paint_magnifier(painter, ic.center(), 6.5, fg),
        Mini::Dot(c) => {
            painter.rect_filled(
                egui::Rect::from_center_size(ic.center(), Vec2::splat(10.0)),
                3,
                c,
            );
        }
    }
    x += 22.0;
    let right = if let Some(n) = count {
        let r = ui.painter().text(
            egui::pos2(rect.right() - 6.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            egui::FontId::proportional(12.0),
            if selected { p.primary } else { p.muted },
        );
        r.left() - 6.0
    } else {
        rect.right() - 6.0
    };
    // Elide long names to the space left.
    let galley = ui
        .painter()
        .layout(label.to_string(), font(13.0), fg, f32::INFINITY);
    let clip =
        egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(right, rect.bottom()));
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    resp.on_hover_text(label)
}

// ------------------------------------------------------------ folder tree

#[derive(Default)]
pub struct Node {
    pub count: usize,
    children: BTreeMap<String, Node>,
}

/// Folder tree of filesystem results, one root per volume.
pub fn build_tree(files: &[FoundFile], vols: &[Volume]) -> Vec<(u16, String, Node)> {
    let mut roots: BTreeMap<u16, Node> = BTreeMap::new();
    for f in files.iter().filter(|f| f.origin != Origin::Carved) {
        let mut n = roots.entry(f.volume).or_default();
        n.count += 1;
        for comp in f.dir.split('/').filter(|c| !c.is_empty()) {
            n = n.children.entry(comp.to_string()).or_default();
            n.count += 1;
        }
    }
    roots
        .into_iter()
        .map(|(v, n)| {
            let name = vols
                .iter()
                .find(|x| x.index == v)
                .map_or_else(|| format!("Volume {v}"), |x| x.desc.clone());
            (v, name, n)
        })
        .collect()
}

struct TreeCtx<'a> {
    sel: &'a mut Option<(u16, String)>,
    changed: bool,
}

fn tree_rec(
    ui: &mut egui::Ui,
    cx: &mut TreeCtx,
    vol: u16,
    path: &str,
    name: &str,
    n: &Node,
    depth: usize,
) {
    let is_sel = cx.sel.as_ref().is_some_and(|(v, p)| *v == vol && p == path);
    let icon = if depth == 0 {
        Mini::Volume
    } else {
        Mini::Folder
    };
    if n.children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            if nav_row(ui, icon, name, Some(n.count), is_sel).clicked() {
                *cx.sel = Some((vol, path.to_string()));
                cx.changed = true;
            }
        });
        return;
    }
    let id = ui.make_persistent_id(("tree", vol, path));
    // Open the first levels and any single-child chain (Users → Name).
    let open = depth < 3 || n.children.len() == 1;
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open)
        .show_header(ui, |ui| {
            if nav_row(ui, icon, name, Some(n.count), is_sel).clicked() {
                *cx.sel = Some((vol, path.to_string()));
                cx.changed = true;
            }
        })
        .body(|ui| {
            for (child, cn) in &n.children {
                let p = if path.is_empty() {
                    child.clone()
                } else {
                    format!("{path}/{child}")
                };
                tree_rec(ui, cx, vol, &p, child, cn, depth + 1);
            }
        });
}

/// Draw the tree; clicking a folder selects `(volume, path)`.
pub fn tree_ui(
    ui: &mut egui::Ui,
    roots: &[(u16, String, Node)],
    sel: &mut Option<(u16, String)>,
) -> bool {
    let mut cx = TreeCtx {
        sel,
        changed: false,
    };
    for (vol, name, n) in roots {
        tree_rec(ui, &mut cx, *vol, "", name, n, 0);
    }
    cx.changed
}

/// Whether `f` lies in the selected tree folder (or below it).
pub fn in_folder(f: &FoundFile, sel: &(u16, String)) -> bool {
    let (vol, path) = sel;
    f.origin != Origin::Carved
        && f.volume == *vol
        && (path.is_empty()
            || &*f.dir == path.as_str()
            || f.dir
                .strip_prefix(path.as_str())
                .is_some_and(|r| r.starts_with('/')))
}

//! "What are you looking for?" — the search form shared by the start screen
//! (before scanning) and the results panel (live filtering), plus the
//! original-folder tree.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use eframe::egui::{self, RichText};
use pulih_core::{canon_ext, parse_date, Category, FoundFile, Origin, Target, Volume};

use crate::i18n::T;
use crate::theme::{ACCENT, MUTED};

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

/// Clickable type chips grouped by category. With `counts`, only types that
/// occur (or are selected) are shown, each with its number of results.
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
            ui.spacing_mut().item_spacing.x = 4.0;
            let all_on = chips.iter().all(|c| c.2.iter().all(|e| exts.contains(*e)));
            let label = format!("{} {}", crate::app::cat_icon(cat), t.cats[cat as usize]);
            if ui
                .add(egui::Button::selectable(
                    all_on,
                    RichText::new(label).strong(),
                ))
                .clicked()
            {
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
                    Some(n) => format!("{name} {n}"),
                    None => name.to_string(),
                };
                if ui
                    .add(egui::Button::selectable(on, RichText::new(text).small()))
                    .clicked()
                {
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

/// The search form. `wide` = start screen layout; otherwise the narrow
/// results side panel. Returns true when anything changed.
pub fn form(
    ui: &mut egui::Ui,
    f: &mut Find,
    counts: Option<&HashMap<String, usize>>,
    t: &T,
    wide: bool,
) -> bool {
    let mut changed = false;
    let label_w = if wide { 120.0 } else { 0.0 };
    let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
        if wide {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [label_w, 22.0],
                    egui::Label::new(RichText::new(label).strong()),
                );
                add(ui);
            });
        } else {
            ui.label(RichText::new(label).strong());
            add(ui);
        }
    };

    row(ui, t.find_name, &mut |ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut f.names)
                .hint_text(t.find_name_hint)
                .desired_width(f32::INFINITY),
        );
        changed |= r.changed();
        r.on_hover_text(t.find_name_help);
    });
    if wide {
        ui.horizontal(|ui| {
            ui.add_space(label_w + 8.0);
            ui.add(egui::Label::new(RichText::new(t.find_name_help).small().color(MUTED)).wrap());
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new(t.find_types).strong());
    changed |= type_picker(ui, &mut f.exts, counts, t);
    ui.add_space(4.0);
    row(ui, t.find_folder, &mut |ui| {
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut f.folder)
                    .hint_text(t.find_folder_hint)
                    .desired_width(f32::INFINITY),
            )
            .changed();
    });
    row(ui, t.find_date, &mut |ui| {
        ui.horizontal_wrapped(|ui| {
            let presets = [
                DatePreset::Any,
                DatePreset::Week,
                DatePreset::Month,
                DatePreset::Year,
                DatePreset::Custom,
            ];
            egui::ComboBox::from_id_salt(("date", wide))
                .selected_text(
                    t.date_presets[presets.iter().position(|p| *p == f.date).unwrap_or(0)],
                )
                .show_ui(ui, |ui| {
                    for (p, l) in presets.iter().zip(t.date_presets) {
                        changed |= ui.selectable_value(&mut f.date, *p, l).changed();
                    }
                });
            if f.date == DatePreset::Custom {
                for (lbl, val) in [(t.find_from, &mut f.from), (t.find_to, &mut f.to)] {
                    ui.label(lbl);
                    let ok = val.trim().is_empty() || parse_date(val).is_some();
                    let r = ui.add(
                        egui::TextEdit::singleline(val)
                            .hint_text(t.date_hint)
                            .desired_width(88.0)
                            .text_color_opt((!ok).then_some(crate::theme::BAD)),
                    );
                    changed |= r.changed();
                }
            }
        });
    });
    row(ui, t.find_size, &mut |ui| {
        ui.horizontal_wrapped(|ui| {
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
    });
    let r = ui
        .checkbox(&mut f.include_unnamed, t.include_unnamed)
        .on_hover_text(t.include_unnamed_hint);
    changed |= r.changed();
    changed
}

/// Short human summary of the active search ("skripsi · DOCX, PDF · 2024").
pub fn summary(f: &Find, t: &T) -> String {
    let mut parts = Vec::new();
    if !f.names.trim().is_empty() {
        parts.push(format!("\u{201c}{}\u{201d}", f.names.trim()));
    }
    if !f.exts.is_empty() {
        let labels: Vec<&str> = TYPES
            .iter()
            .filter(|c| c.2.iter().any(|e| f.exts.contains(*e)))
            .map(|c| c.1)
            .collect();
        parts.push(labels.join(", "));
    }
    if !f.folder.trim().is_empty() {
        parts.push(format!("📁 {}", f.folder.trim()));
    }
    if f.date != DatePreset::Any {
        let i = [
            DatePreset::Any,
            DatePreset::Week,
            DatePreset::Month,
            DatePreset::Year,
            DatePreset::Custom,
        ]
        .iter()
        .position(|p| *p == f.date)
        .unwrap_or(0);
        parts.push(if f.date == DatePreset::Custom {
            format!("{} – {}", f.from.trim(), f.to.trim())
        } else {
            t.date_presets[i].to_string()
        });
    }
    if f.min_mb > 0.0 || f.max_mb > 0.0 {
        parts.push(format!(
            "{}–{} MB",
            f.min_mb,
            if f.max_mb > 0.0 {
                f.max_mb.to_string()
            } else {
                "∞".into()
            }
        ));
    }
    parts.join("  ·  ")
}

/// Number of results per canonical extension.
pub fn ext_counts(files: &[FoundFile]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for f in files {
        *m.entry(canon_ext(&f.ext)).or_insert(0) += 1;
    }
    m
}

// ------------------------------------------------------------ folder tree

#[derive(Default)]
pub struct Node {
    count: usize,
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

/// Draw the tree; clicking a folder selects `(volume, path)`.
pub fn tree_ui(
    ui: &mut egui::Ui,
    roots: &[(u16, String, Node)],
    sel: &mut Option<(u16, String)>,
) -> bool {
    let mut changed = false;
    #[allow(clippy::too_many_arguments)]
    fn rec(
        ui: &mut egui::Ui,
        vol: u16,
        path: &str,
        name: &str,
        n: &Node,
        sel: &mut Option<(u16, String)>,
        changed: &mut bool,
        depth: usize,
    ) {
        let is_sel = sel.as_ref().is_some_and(|(v, p)| *v == vol && p == path);
        let label = RichText::new(format!(
            "{} {name}  ({})",
            if depth == 0 { "💽" } else { "📁" },
            n.count
        ));
        let label = if is_sel {
            label.strong().color(ACCENT)
        } else {
            label
        };
        if n.children.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if ui.selectable_label(is_sel, label).clicked() {
                    *sel = Some((vol, path.to_string()));
                    *changed = true;
                }
            });
            return;
        }
        let id = ui.make_persistent_id(("tree", vol, path));
        // Open the first levels and any single-child chain (Users → Name).
        let open = depth < 3 || n.children.len() == 1;
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open)
            .show_header(ui, |ui| {
                if ui.selectable_label(is_sel, label).clicked() {
                    *sel = Some((vol, path.to_string()));
                    *changed = true;
                }
            })
            .body(|ui| {
                for (child, cn) in &n.children {
                    let p = if path.is_empty() {
                        child.clone()
                    } else {
                        format!("{path}/{child}")
                    };
                    rec(ui, vol, &p, child, cn, sel, changed, depth + 1);
                }
            });
    }
    for (vol, name, n) in roots {
        rec(ui, *vol, "", name, n, sel, &mut changed, 0);
    }
    changed
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

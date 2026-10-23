use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver};
use eframe::egui::{self, Align, Color32, Id, Layout, RichText, Sense, TextureOptions, Vec2};
use egui_extras::{Column, TableBuilder};
use pulih_core::bytes::{fmt_size, fmt_time};
use pulih_core::{
    list_devices, recover, same_disk, start_scan, Category, Condition, Device, DeviceInfo,
    DeviceKind, FileData, FoundFile, Inspection, Origin, Phase, ReadAt, RecoverOptions,
    RecoverProgress, RecoverReport, ScanEvent, ScanHandle, ScanOptions, ScanSummary, Volume,
};

use crate::find::{self, Find};
use crate::i18n::{Lang, T};
use crate::preview::{Preview, Previewer};
use crate::theme::{ACCENT, BAD, GOOD, MUTED, WARN};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Both,
    Quick,
    Deep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Folder,
    Type,
    Size,
    Modified,
    Condition,
    Source,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OriginFilter {
    All,
    Fs,
    Carved,
}

#[derive(PartialEq, Eq)]
enum Screen {
    Source,
    Results,
}

/// Result-only filters (the search itself lives in [`Find`]).
struct Filter {
    good_only: bool,
    origin: OriginFilter,
}

enum PreviewState {
    Loading,
    Image(egui::TextureHandle, (u32, u32)),
    Text(String),
    Hex(String),
}

struct RecoverJob {
    progress: Arc<RecoverProgress>,
    rx: Receiver<RecoverReport>,
    report: Option<RecoverReport>,
    out: PathBuf,
    started: Instant,
}

pub struct App {
    lang: Lang,
    screen: Screen,
    devices: Vec<DeviceInfo>,
    selected_path: Option<String>,
    path_input: String,
    mode: Mode,
    opts: ScanOptions,
    error: Option<String>,

    source: Option<Arc<Device>>,
    source_label: String,
    scan: Option<ScanHandle>,
    vols: Vec<Volume>,
    summary: Option<ScanSummary>,
    logs: Vec<String>,

    files: Vec<FoundFile>,
    checked: HashSet<u64>,
    checked_bytes: u64,
    view: Vec<u32>,
    view_dirty: bool,
    last_build: Instant,
    counts: [usize; 6],
    filter: Filter,
    find: Find,
    ext_counts: HashMap<String, usize>,
    tree: Vec<(u16, String, find::Node)>,
    tree_sel: Option<(u16, String)>,
    stats_dirty: bool,
    /// Types the running/last deep scan was limited to.
    deep_limit: Option<Vec<String>>,
    sort: (Col, bool),
    focus: Option<u64>,
    scroll_to: Option<usize>,

    previewer: Previewer,
    preview: Option<(u64, PreviewState)>,
    inspection: Option<(u64, Inspection)>,

    recover: Option<RecoverJob>,
    keep_folders: bool,
    out_dir: String,
    confirm_same_disk: Option<(PathBuf, Vec<u64>)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let lang = match std::env::var("LANG")
            .unwrap_or_default()
            .to_ascii_lowercase()
        {
            l if l.starts_with("en") => Lang::En,
            _ => Lang::Id,
        };
        App {
            lang,
            screen: Screen::Source,
            devices: list_devices(),
            selected_path: None,
            path_input: String::new(),
            mode: Mode::Both,
            opts: ScanOptions::default(),
            error: None,
            source: None,
            source_label: String::new(),
            scan: None,
            vols: Vec::new(),
            summary: None,
            logs: Vec::new(),
            files: Vec::new(),
            checked: HashSet::new(),
            checked_bytes: 0,
            view: Vec::new(),
            view_dirty: false,
            last_build: Instant::now(),
            counts: [0; 6],
            filter: Filter {
                good_only: false,
                origin: OriginFilter::All,
            },
            find: Find::default(),
            ext_counts: HashMap::new(),
            tree: Vec::new(),
            tree_sel: None,
            stats_dirty: false,
            deep_limit: None,
            sort: (Col::Condition, true),
            focus: None,
            scroll_to: None,
            previewer: Previewer::default(),
            preview: None,
            inspection: None,
            recover: None,
            keep_folders: true,
            out_dir: String::new(),
            confirm_same_disk: None,
        }
    }

    fn t(&self) -> &'static T {
        self.lang.t()
    }

    fn scanning(&self) -> bool {
        self.scan.is_some() && self.summary.is_none()
    }

    // ------------------------------------------------------------------ state

    fn pump(&mut self, ctx: &egui::Context) {
        if let Some(h) = &self.scan {
            let events: Vec<ScanEvent> = h.events.try_iter().collect();
            let finished = h.is_finished();
            let mut changed = false;
            for ev in events {
                match ev {
                    ScanEvent::Volumes(v) => self.vols = v,
                    ScanEvent::Found(v) => {
                        for f in &v {
                            self.counts[f.category as usize] += 1;
                        }
                        self.files.extend(v);
                        changed = true;
                    }
                    ScanEvent::CarveFinal(v) => {
                        self.files.retain(|f| f.origin != Origin::Carved);
                        self.files.extend(v);
                        self.counts = [0; 6];
                        for f in &self.files {
                            self.counts[f.category as usize] += 1;
                        }
                        let ids: HashSet<u64> = self.files.iter().map(|f| f.id).collect();
                        self.checked.retain(|id| ids.contains(id));
                        self.recount_checked();
                        changed = true;
                    }
                    ScanEvent::Log(s) => self.logs.push(s),
                    ScanEvent::Done(s) => self.summary = Some(s),
                }
            }
            if changed {
                self.view_dirty = true;
                self.stats_dirty = true;
            }
            if self.summary.is_some() && finished {
                if let Some(h) = self.scan.take() {
                    h.wait();
                }
                self.view_dirty = true;
            }
        }
        // Rebuilding a million-row view is cheap but not free: throttle it
        // while results are still streaming in.
        if self.view_dirty
            && (!self.scanning() || self.last_build.elapsed() > Duration::from_millis(400))
        {
            self.rebuild_view();
        }
        for (id, p, ins) in self.previewer.rx.try_iter() {
            if self.preview.as_ref().is_some_and(|(pid, _)| *pid == id) {
                self.inspection = Some((id, ins));
                let st = match p {
                    Preview::Image { w, h, rgba, full } => {
                        let img = egui::ColorImage::from_rgba_unmultiplied(
                            [w as usize, h as usize],
                            &rgba,
                        );
                        PreviewState::Image(
                            ctx.load_texture(format!("pv{id}"), img, TextureOptions::LINEAR),
                            full,
                        )
                    }
                    Preview::Text(s) => PreviewState::Text(s),
                    Preview::Hex(s) => PreviewState::Hex(s),
                };
                self.preview = Some((id, st));
            }
        }
        if let Some(j) = &mut self.recover {
            if let Ok(r) = j.rx.try_recv() {
                j.report = Some(r);
            }
        }
    }

    fn recount_checked(&mut self) {
        self.checked_bytes = self
            .files
            .iter()
            .filter(|f| self.checked.contains(&f.id))
            .map(|f| f.size)
            .sum();
    }

    fn rebuild_view(&mut self) {
        if self.stats_dirty {
            self.ext_counts = find::ext_counts(&self.files);
            self.tree = find::build_tree(&self.files, &self.vols);
            self.stats_dirty = false;
        }
        let target = self.find.target();
        let flt = &self.filter;
        let sel = &self.tree_sel;
        let mut view: Vec<u32> = (0..self.files.len() as u32)
            .filter(|&i| {
                let f = &self.files[i as usize];
                (!flt.good_only || f.condition == Condition::Good)
                    && match flt.origin {
                        OriginFilter::All => true,
                        OriginFilter::Fs => f.origin != Origin::Carved,
                        OriginFilter::Carved => f.origin == Origin::Carved,
                    }
                    && sel.as_ref().is_none_or(|s| find::in_folder(f, s))
                    && target.matches(f)
            })
            .collect();
        let files = &self.files;
        let (col, asc) = self.sort;
        view.sort_by(|&a, &b| {
            let (x, y) = (&files[a as usize], &files[b as usize]);
            let o = match col {
                Col::Name => icmp(&x.name, &y.name),
                Col::Folder => icmp(&x.dir, &y.dir).then_with(|| icmp(&x.name, &y.name)),
                Col::Type => x.ext.cmp(&y.ext),
                Col::Size => x.size.cmp(&y.size),
                Col::Modified => x.modified.cmp(&y.modified),
                Col::Condition => x
                    .condition
                    .cmp(&y.condition)
                    .then_with(|| x.origin.cmp(&y.origin))
                    .then_with(|| icmp(&x.dir, &y.dir))
                    .then_with(|| icmp(&x.name, &y.name)),
                Col::Source => x.origin.cmp(&y.origin),
            };
            let o = o.then_with(|| x.first_offset().cmp(&y.first_offset()));
            if asc {
                o
            } else {
                o.reverse()
            }
        });
        self.view = view;
        self.view_dirty = false;
        self.last_build = Instant::now();
    }

    fn start_scan(&mut self) {
        let t = self.t();
        let path = match self.selected_path.clone() {
            Some(p) => p,
            None if !self.path_input.trim().is_empty() => self.path_input.trim().to_string(),
            None => {
                self.error = Some(t.pick_source_first.into());
                return;
            }
        };
        let dev = match Device::open(&path) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                self.error = Some(if e.kind() == std::io::ErrorKind::PermissionDenied {
                    t.need_admin.to_string()
                } else {
                    format!("{} {path}: {e}", t.open_failed)
                });
                return;
            }
        };
        let mut o = self.opts.clone();
        o.categories = 0x3F;
        o.only_exts = self.find.target().wanted_exts();
        self.deep_limit = o.only_exts.clone().filter(|_| self.mode != Mode::Quick);
        (o.quick, o.deep) = match self.mode {
            Mode::Both => (true, true),
            Mode::Quick => (true, false),
            Mode::Deep => (false, true),
        };
        self.source_label = self.devices.iter().find(|d| d.path == path).map_or_else(
            || path.clone(),
            |d| format!("{} ({})", d.name, fmt_size(d.size)),
        );
        self.error = None;
        self.files.clear();
        self.checked.clear();
        self.checked_bytes = 0;
        self.view.clear();
        self.counts = [0; 6];
        self.logs.clear();
        self.summary = None;
        self.vols.clear();
        self.focus = None;
        self.preview = None;
        self.tree.clear();
        self.tree_sel = None;
        self.ext_counts.clear();
        self.scan = Some(start_scan(dev.clone(), o));
        self.source = Some(dev);
        self.screen = Screen::Results;
    }

    fn set_focus(&mut self, idx: usize, ctx: &egui::Context) {
        let f = &self.files[idx];
        if self.focus == Some(f.id) {
            return;
        }
        self.focus = Some(f.id);
        if let Some(src) = &self.source {
            self.preview = Some((f.id, PreviewState::Loading));
            self.previewer.request(src.clone(), f.clone(), ctx.clone());
        }
    }

    fn pick_out_dir(&mut self) -> bool {
        match rfd::FileDialog::new()
            .set_title(self.t().choose_out)
            .pick_folder()
        {
            Some(d) => {
                self.out_dir = d.to_string_lossy().into_owned();
                true
            }
            None => false,
        }
    }

    fn begin_recover(&mut self, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        if self.out_dir.trim().is_empty() && !self.pick_out_dir() {
            return;
        }
        let dir = PathBuf::from(self.out_dir.trim());
        let src_path = self
            .source
            .as_ref()
            .map(|s| s.path.clone())
            .unwrap_or_default();
        if same_disk(&src_path, &dir) {
            self.confirm_same_disk = Some((dir, ids));
            return;
        }
        self.run_recover(dir, ids);
    }

    fn run_recover(&mut self, dir: PathBuf, ids: Vec<u64>) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let want: HashSet<u64> = ids.into_iter().collect();
        let files: Vec<FoundFile> = self
            .files
            .iter()
            .filter(|f| want.contains(&f.id))
            .cloned()
            .collect();
        let vols = self.vols.clone();
        let progress = Arc::new(RecoverProgress::default());
        let (tx, rx) = bounded(1);
        let opts = RecoverOptions {
            keep_folders: self.keep_folders,
            ..Default::default()
        };
        let p = progress.clone();
        let out = dir.clone();
        std::thread::spawn(move || {
            let r = recover(&*src as &dyn ReadAt, &files, &vols, &out, &opts, &p);
            let _ = tx.send(r);
        });
        self.recover = Some(RecoverJob {
            progress,
            rx,
            report: None,
            out: dir,
            started: Instant::now(),
        });
    }

    // --------------------------------------------------------------- widgets

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Pulih").size(24.0).strong().color(ACCENT));
                ui.label(RichText::new(t.subtitle).color(MUTED));
                if self.screen == Screen::Results && !self.source_label.is_empty() {
                    ui.separator();
                    ui.label(RichText::new(format!("💽 {}", self.source_label)).strong());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                    ui.separator();
                    ui.selectable_value(&mut self.lang, Lang::En, "EN");
                    ui.selectable_value(&mut self.lang, Lang::Id, "ID");
                });
            });
            ui.add_space(4.0);
        });
    }

    fn source_screen(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        egui::Panel::bottom("start").show(ui, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(980.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    egui::Frame::new()
                        .fill(WARN.gamma_multiply(0.15))
                        .stroke(egui::Stroke::new(1.0, WARN.gamma_multiply(0.6)))
                        .corner_radius(8)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(t.warn_write);
                        });
                    if let Some(e) = &self.error {
                        ui.add_space(6.0);
                        ui.colored_label(BAD, e);
                    }
                    ui.add_space(8.0);
                    let ready = self.selected_path.is_some() || !self.path_input.trim().is_empty();
                    let btn = egui::Button::new(
                        RichText::new(t.start)
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if ready { ACCENT } else { MUTED })
                    .min_size(Vec2::new(220.0, 40.0));
                    if ui.add(btn).clicked()
                        || (ready && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        self.start_scan();
                    }
                });
            });
            ui.add_space(10.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(980.0);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.add_space(8.0);
                            ui.heading(t.step_source);
                            ui.label(RichText::new(t.step_source_hint).color(MUTED));
                            ui.add_space(6.0);
                            self.device_list(ui);
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui.button(t.refresh).clicked() {
                                    self.devices = list_devices();
                                }
                                if ui.button(t.open_image).clicked() {
                                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                                        let p = p.to_string_lossy().into_owned();
                                        self.path_input = p.clone();
                                        self.selected_path = Some(p);
                                    }
                                }
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut self.path_input)
                                        .hint_text(t.image_hint)
                                        .desired_width(f32::INFINITY),
                                );
                                if r.changed() {
                                    self.selected_path = None;
                                }
                            });
                            ui.label(RichText::new(t.drop_hint).small().color(MUTED));

                            ui.add_space(14.0);
                            ui.heading(t.step_find);
                            ui.label(RichText::new(t.step_find_hint).color(MUTED));
                            ui.add_space(4.0);
                            egui::Frame::group(ui.style())
                                .corner_radius(8)
                                .inner_margin(10)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    find::form(ui, &mut self.find, None, t, true);
                                    if let Some(e) = self.find.target().wanted_exts() {
                                        if self.mode != Mode::Quick {
                                            ui.label(
                                                RichText::new(format!(
                                                    "🔎 {}: {}",
                                                    t.deep_limited,
                                                    e.join(", ").to_uppercase()
                                                ))
                                                .small()
                                                .color(ACCENT),
                                            );
                                        }
                                    }
                                });

                            ui.add_space(14.0);
                            ui.heading(t.step_options);
                            ui.add_space(4.0);
                            ui.columns(3, |cols| {
                                for (i, (m, title, hint)) in [
                                    (Mode::Both, t.mode_both, t.mode_both_hint),
                                    (Mode::Quick, t.mode_quick, t.mode_quick_hint),
                                    (Mode::Deep, t.mode_deep, t.mode_deep_hint),
                                ]
                                .into_iter()
                                .enumerate()
                                {
                                    if mode_card(&mut cols[i], self.mode == m, title, hint) {
                                        self.mode = m;
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            if self.mode != Mode::Quick {
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(&mut self.opts.free_only, t.free_only)
                                        .on_hover_text(t.free_only_hint);
                                    ui.checkbox(&mut self.opts.byte_level, t.byte_level)
                                        .on_hover_text(t.byte_level_hint);
                                });
                                egui::CollapsingHeader::new(t.advanced).show(ui, |ui| {
                                    ui.add(
                                        egui::Slider::new(&mut self.opts.threads, 1..=16)
                                            .text(t.threads),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut self.opts.chunk_mb, 4..=64)
                                            .text(t.chunk),
                                    );
                                });
                            }
                            ui.add_space(16.0);
                        });
                    });
                });
        });
    }

    fn device_list(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        if self.devices.is_empty() {
            ui.colored_label(WARN, t.no_devices);
            return;
        }
        egui::Frame::group(ui.style())
            .corner_radius(8)
            .inner_margin(6)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for d in self.devices.clone() {
                    let sel = self.selected_path.as_deref() == Some(d.path.as_str());
                    let indent = if d.kind == DeviceKind::Partition && d.parent.is_some() {
                        26.0
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        let icon = match (d.kind, d.removable) {
                            (DeviceKind::Disk, true) => "💾",
                            (DeviceKind::Disk, false) => "🖴",
                            (DeviceKind::Partition, _) => "💽",
                        };
                        let mut text = format!("{icon}  {}", d.name);
                        if !d.mount.is_empty() {
                            text.push_str(&format!("   [{}]", d.mount));
                        }
                        let w = ui.available_width() - 210.0;
                        let r = ui.add_sized(
                            [w.max(120.0), 26.0],
                            egui::Button::selectable(sel, RichText::new(text).strong())
                                .right_text(""),
                        );
                        if r.clicked() {
                            self.selected_path = Some(d.path.clone());
                            self.path_input = d.path.clone();
                            self.error = None;
                        }
                        if r.double_clicked() {
                            self.start_scan();
                        }
                        ui.label(RichText::new(fmt_size(d.size)).monospace());
                        if !d.fs.is_empty() {
                            ui.label(RichText::new(&d.fs).color(ACCENT));
                        }
                        if d.removable {
                            ui.label(RichText::new(t.removable).small().color(MUTED));
                        }
                    });
                }
            });
    }

    fn results_screen(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let ctx = ui.ctx().clone();
        self.status_bar(ui);

        egui::Panel::left("filters")
            .resizable(true)
            .default_size(250.0)
            .min_size(200.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        if self.find.is_active() || self.tree_sel.is_some() {
                            egui::Frame::new()
                                .fill(ACCENT.gamma_multiply(0.12))
                                .corner_radius(6)
                                .inner_margin(6)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(format!(
                                            "🔎 {} — {} {}",
                                            t.search_active,
                                            fmt_num(self.view.len(), self.lang),
                                            t.matching
                                        ))
                                        .strong(),
                                    );
                                    let mut s = find::summary(&self.find, t);
                                    if let Some((_, p)) = &self.tree_sel {
                                        let folder = format!(
                                            "📁 {}",
                                            if p.is_empty() { "/" } else { p.as_str() }
                                        );
                                        s = if s.is_empty() {
                                            folder
                                        } else {
                                            format!("{s}  ·  {folder}")
                                        };
                                    }
                                    if !s.is_empty() {
                                        ui.label(RichText::new(s).small());
                                    }
                                    if ui.button(t.reset_search).clicked() {
                                        self.find = Find::default();
                                        self.tree_sel = None;
                                        self.view_dirty = true;
                                    }
                                });
                            ui.add_space(4.0);
                        }
                        if find::form(ui, &mut self.find, Some(&self.ext_counts), t, false) {
                            self.view_dirty = true;
                        }
                        ui.separator();
                        egui::CollapsingHeader::new(
                            RichText::new(format!("📁 {}", t.find_folder)).strong(),
                        )
                        .default_open(true)
                        .show(ui, |ui| {
                            if ui
                                .selectable_label(self.tree_sel.is_none(), t.all_folders)
                                .clicked()
                            {
                                self.tree_sel = None;
                                self.view_dirty = true;
                            }
                            if find::tree_ui(ui, &self.tree, &mut self.tree_sel) {
                                self.view_dirty = true;
                            }
                        });
                        ui.separator();
                        ui.label(RichText::new(t.filters).strong());
                        if ui
                            .checkbox(&mut self.filter.good_only, t.only_good)
                            .changed()
                        {
                            self.view_dirty = true;
                        }
                        for (o, l) in [
                            (OriginFilter::All, t.origin_all),
                            (OriginFilter::Fs, t.origin_fs),
                            (OriginFilter::Carved, t.origin_carved),
                        ] {
                            if ui.radio_value(&mut self.filter.origin, o, l).changed() {
                                self.view_dirty = true;
                            }
                        }
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(t.select_all).clicked() {
                                for &i in &self.view {
                                    self.checked.insert(self.files[i as usize].id);
                                }
                                self.recount_checked();
                            }
                            if ui.button(t.select_none).clicked() {
                                self.checked.clear();
                                self.checked_bytes = 0;
                            }
                        });
                        if !self.logs.is_empty() {
                            ui.separator();
                            egui::CollapsingHeader::new(t.log).show(ui, |ui| {
                                for l in &self.logs {
                                    ui.label(RichText::new(l).small());
                                }
                            });
                        }
                    });
            });

        egui::Panel::right("preview")
            .resizable(true)
            .default_size(330.0)
            .min_size(240.0)
            .show(ui, |ui| {
                self.preview_panel(ui);
            });

        egui::Panel::bottom("actions").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {} {} {}",
                    fmt_num(self.view.len(), self.lang),
                    t.of,
                    fmt_num(self.files.len(), self.lang),
                    t.shown,
                ));
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        fmt_num(self.checked.len(), self.lang),
                        t.selected
                    ))
                    .strong(),
                );
                ui.label(
                    RichText::new(fmt_size(self.checked_bytes))
                        .strong()
                        .color(ACCENT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut self.keep_folders, t.keep_folders);
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t.save_to);
                let enabled = !self.checked.is_empty() && self.recover.is_none();
                let mut go = false;
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let btn = egui::Button::new(
                        RichText::new(t.recover)
                            .size(15.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if enabled { ACCENT } else { MUTED })
                    .min_size(Vec2::new(150.0, 32.0));
                    if ui.add_enabled(enabled, btn).clicked() {
                        go = true;
                    }
                    if ui.button("📂").on_hover_text(t.choose_out).clicked() {
                        self.pick_out_dir();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.out_dir)
                            .hint_text(t.save_to_hint)
                            .desired_width(ui.available_width()),
                    );
                });
                if go {
                    let ids: Vec<u64> = self.checked.iter().copied().collect();
                    self.begin_recover(ids);
                }
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.view.is_empty() {
                ui.centered_and_justified(|ui| {
                    let msg = if self.scanning() {
                        t.empty_scanning
                    } else if self.find.is_active() || self.tree_sel.is_some() {
                        t.nothing_matches
                    } else {
                        t.empty_results
                    };
                    ui.label(RichText::new(msg).color(MUTED));
                });
                return;
            }
            self.table(ui, &ctx);
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let lang = self.lang;
        egui::Panel::top("status").show(ui, |ui| {
            ui.add_space(6.0);
            if let Some(h) = &self.scan {
                let s = h.progress.snapshot();
                let phase = match s.phase {
                    Phase::Analyzing | Phase::Idle => t.phase_analyzing,
                    Phase::Filesystem => t.phase_fs,
                    Phase::Carving => t.phase_carving,
                    Phase::Done => t.phase_done,
                    Phase::Cancelled => t.phase_cancelled,
                };
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new(phase).strong());
                    if !s.status.is_empty() {
                        ui.label(RichText::new(&s.status).color(MUTED).small());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t.stop).clicked() {
                            h.cancel();
                        }
                    });
                });
                ui.add(
                    egui::ProgressBar::new(s.fraction())
                        .fill(ACCENT)
                        .desired_height(14.0)
                        .text(format!(
                            "{:.1}%  —  {} / {}",
                            s.fraction() * 100.0,
                            fmt_size(s.done),
                            fmt_size(s.total)
                        )),
                );
                ui.horizontal(|ui| {
                    stat(ui, t.speed, &format!("{}/s", fmt_size(s.rate() as u64)));
                    stat(ui, t.eta, &s.eta().map_or("—".into(), fmt_dur));
                    stat(ui, t.elapsed, &fmt_dur(s.elapsed));
                    stat(ui, t.found, &fmt_num(s.found as usize, lang));
                    if s.bad_sectors > 0 {
                        stat(ui, t.bad_sectors, &fmt_num(s.bad_sectors as usize, lang));
                    }
                });
            } else if let Some(s) = &self.summary {
                ui.horizontal(|ui| {
                    let (icon, phase) = if s.cancelled {
                        ("⏹", t.phase_cancelled)
                    } else {
                        ("✔", t.phase_done)
                    };
                    ui.label(
                        RichText::new(format!("{icon} {phase}"))
                            .strong()
                            .color(if s.cancelled { WARN } else { GOOD }),
                    );
                    stat(ui, t.elapsed, &fmt_dur(s.elapsed));
                    stat(ui, t.found, &fmt_num(self.files.len(), lang));
                    stat(ui, t.origin_fs, &fmt_num(s.fs_found, lang));
                    stat(ui, t.origin_carved, &fmt_num(s.carved, lang));
                    if let Some(e) = &self.deep_limit {
                        ui.label(
                            RichText::new(format!(
                                "🔎 {}: {}",
                                t.deep_limited,
                                e.join(", ").to_uppercase()
                            ))
                            .small()
                            .color(ACCENT),
                        );
                    }
                    if s.bad_sectors > 0 {
                        stat(ui, t.bad_sectors, &fmt_num(s.bad_sectors as usize, lang));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(t.new_scan).clicked() {
                            self.screen = Screen::Source;
                            self.devices = list_devices();
                        }
                    });
                });
            }
            ui.add_space(6.0);
        });
    }

    fn table(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.t();
        let lang = self.lang;
        // Keyboard navigation: ↑/↓ moves the preview, Space ticks.
        let pos = self.focus.and_then(|id| {
            self.view
                .iter()
                .position(|&i| self.files[i as usize].id == id)
        });
        let (up, down, space) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Space),
            )
        });
        let typing = ctx.egui_wants_keyboard_input();
        if !typing && (up || down) && !self.view.is_empty() {
            let n = match pos {
                Some(p) if up => p.saturating_sub(1),
                Some(p) => (p + 1).min(self.view.len() - 1),
                None => 0,
            };
            let idx = self.view[n] as usize;
            self.set_focus(idx, ctx);
            self.scroll_to = Some(n);
        }
        if !typing && space {
            if let Some(id) = self.focus {
                if !self.checked.remove(&id) {
                    self.checked.insert(id);
                }
                self.recount_checked();
            }
        }

        let mut toggled: Option<u64> = None;
        let mut clicked: Option<usize> = None;
        let mut sort_by: Option<Col> = None;
        let files = &self.files;
        let view = &self.view;
        let checked = &self.checked;
        let focus = self.focus;
        let (sort_col, asc) = self.sort;
        let all_checked = !view.is_empty()
            && view
                .iter()
                .all(|&i| checked.contains(&files[i as usize].id));
        let mut check_all: Option<bool> = None;

        // Plain (non-selectable) labels so a click anywhere selects the row.
        ui.style_mut().interaction.selectable_labels = false;
        let mut tb = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(26.0))
            .column(Column::remainder().at_least(140.0).clip(true))
            .column(Column::initial(150.0).at_least(60.0).clip(true))
            .column(Column::initial(48.0).at_least(40.0))
            .column(Column::initial(74.0).at_least(60.0))
            .column(Column::initial(112.0).at_least(80.0))
            .column(Column::initial(96.0).at_least(70.0))
            .column(Column::initial(76.0).at_least(60.0).clip(true));
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, None);
        }
        tb.header(26.0, |mut h| {
            h.col(|ui| {
                let mut a = all_checked;
                if ui.checkbox(&mut a, "").changed() {
                    check_all = Some(a);
                }
            });
            for (c, label) in [
                (Col::Name, t.col_name),
                (Col::Folder, t.col_folder),
                (Col::Type, t.col_type),
                (Col::Size, t.col_size),
                (Col::Modified, t.col_modified),
                (Col::Condition, t.col_condition),
                (Col::Source, t.col_source),
            ] {
                h.col(|ui| {
                    let arrow = if sort_col == c {
                        if asc {
                            " ⏶"
                        } else {
                            " ⏷"
                        }
                    } else {
                        ""
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(format!("{label}{arrow}")).strong())
                                .frame(false),
                        )
                        .clicked()
                    {
                        sort_by = Some(c);
                    }
                });
            }
        })
        .body(|body| {
            body.rows(22.0, view.len(), |mut row| {
                let idx = view[row.index()] as usize;
                let f = &files[idx];
                row.set_selected(focus == Some(f.id));
                row.col(|ui| {
                    let mut c = checked.contains(&f.id);
                    if ui.checkbox(&mut c, "").changed() {
                        toggled = Some(f.id);
                    }
                });
                row.col(|ui| {
                    ui.label(format!("{} {}", cat_icon(f.category), f.name));
                    if let Some(i) = &f.info {
                        ui.label(RichText::new(i).small().color(MUTED));
                    }
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(if f.dir.is_empty() { "—" } else { &f.dir }).color(MUTED),
                    );
                });
                row.col(|ui| {
                    ui.label(RichText::new(f.ext.to_ascii_uppercase()).small());
                });
                row.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(fmt_size(f.size))
                    });
                });
                row.col(|ui| {
                    ui.label(RichText::new(f.modified.map_or("—".into(), fmt_time)).small());
                });
                row.col(|ui| {
                    let (c, txt) = cond_style(f.condition, t);
                    ui.label(RichText::new(txt).color(c))
                        .on_hover_text(t.cond_hint[f.condition as usize]);
                });
                row.col(|ui| {
                    ui.label(
                        RichText::new(origin_label(f.origin, t))
                            .small()
                            .color(MUTED),
                    );
                });
                if row.response().clicked() {
                    clicked = Some(idx);
                }
            });
        });

        if let Some(id) = toggled {
            if !self.checked.remove(&id) {
                self.checked.insert(id);
            }
            self.recount_checked();
        }
        if let Some(a) = check_all {
            for &i in &self.view {
                let id = self.files[i as usize].id;
                if a {
                    self.checked.insert(id);
                } else {
                    self.checked.remove(&id);
                }
            }
            self.recount_checked();
        }
        if let Some(i) = clicked {
            self.set_focus(i, ctx);
        }
        if let Some(c) = sort_by {
            self.sort = if self.sort.0 == c {
                (c, !self.sort.1)
            } else {
                (c, c != Col::Size && c != Col::Modified)
            };
            self.view_dirty = true;
        }
        let _ = lang;
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        ui.add_space(6.0);
        ui.label(RichText::new(t.preview).strong());
        let Some(f) = self
            .focus
            .and_then(|id| self.files.iter().find(|f| f.id == id))
            .cloned()
        else {
            ui.add_space(20.0);
            ui.label(RichText::new("—").color(MUTED));
            return;
        };
        let ins: Option<Inspection> = self
            .inspection
            .as_ref()
            .filter(|(id, _)| *id == f.id)
            .map(|(_, i)| i.clone());
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let w = ui.available_width();
                match self
                    .preview
                    .as_ref()
                    .filter(|(id, _)| *id == f.id)
                    .map(|p| &p.1)
                {
                    Some(PreviewState::Loading) => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new());
                            ui.label(t.loading);
                        });
                    }
                    Some(PreviewState::Image(tex, full)) => {
                        let s = tex.size_vec2();
                        let scale = (w / s.x).min(360.0 / s.y).min(1.5);
                        ui.add(egui::Image::new((tex.id(), s * scale)).corner_radius(6));
                        ui.label(
                            RichText::new(format!("{} × {} px", full.0, full.1))
                                .small()
                                .color(MUTED),
                        );
                    }
                    Some(PreviewState::Text(s)) => {
                        let is_doc = ins.as_ref().is_some_and(|i| i.text.is_some());
                        if is_doc {
                            ui.label(
                                RichText::new(format!("📄 {}", t.doc_text))
                                    .small()
                                    .color(MUTED),
                            );
                        }
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("doctext")
                                .max_height(300.0)
                                .show(ui, |ui| {
                                    let text = if is_doc {
                                        RichText::new(s)
                                    } else {
                                        RichText::new(s).monospace().small()
                                    };
                                    ui.add(egui::Label::new(text).wrap());
                                });
                        });
                    }
                    Some(PreviewState::Hex(s)) => {
                        ui.label(RichText::new(t.no_preview).small().color(MUTED));
                        egui::ScrollArea::horizontal()
                            .id_salt("hex")
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(s).monospace().size(10.5))
                                        .extend(),
                                );
                            });
                    }
                    None => {}
                }
                ui.add_space(8.0);
                ui.label(RichText::new(&f.name).strong().size(15.0));
                let (cc, ctext) = cond_style(f.condition, t);
                ui.label(RichText::new(ctext).color(cc).strong());
                ui.label(
                    RichText::new(t.cond_hint[f.condition as usize])
                        .small()
                        .color(MUTED),
                );
                ui.add_space(6.0);
                egui::Grid::new("details")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        let mut kv = |k: &str, v: String| {
                            ui.label(RichText::new(k).color(MUTED));
                            ui.add(egui::Label::new(v).wrap());
                            ui.end_row();
                        };
                        kv(
                            t.col_size,
                            format!(
                                "{} ({} B)",
                                fmt_size(f.size),
                                fmt_num(f.size as usize, self.lang)
                            ),
                        );
                        kv(t.col_type, f.ext.to_ascii_uppercase());
                        if !f.dir.is_empty() {
                            kv(t.col_folder, f.dir.to_string());
                        }
                        if let Some(m) = f.modified {
                            kv(t.col_modified, fmt_time(m));
                        }
                        if let Some(i) = &ins {
                            if let Some(title) =
                                i.title.as_ref().filter(|x| !f.name.starts_with(x.as_str()))
                            {
                                kv(t.content_title, title.clone());
                            }
                            if let Some(tm) = i.time.filter(|tm| Some(*tm) != f.modified) {
                                kv(t.content_date, fmt_time(tm));
                            }
                            if let Some(info) = i.info.as_ref().filter(|_| f.info.is_none()) {
                                kv(t.info, info.clone());
                            }
                        }
                        if let Some(i) = &f.info {
                            kv(t.info, i.clone());
                        }
                        kv(t.col_source, origin_label(f.origin, t).to_string());
                        let vol = self
                            .vols
                            .iter()
                            .find(|v| v.index == f.volume)
                            .map_or_else(|| t.whole_device.to_string(), |v| v.desc.clone());
                        kv(t.volume, vol);
                        let off = f.first_offset();
                        if off != u64::MAX {
                            kv(t.offset, format!("{off} (0x{off:X})"));
                        }
                        if let FileData::Extents(e) = &f.data {
                            kv(t.fragments, e.len().to_string());
                        }
                    });
                if f.origin == Origin::Carved {
                    ui.add_space(4.0);
                    let named = !f.name.starts_with('f') || f.name.contains(' ');
                    ui.label(
                        RichText::new(if named {
                            t.named_from_content
                        } else {
                            t.carved_name_note
                        })
                        .small()
                        .italics()
                        .color(MUTED),
                    );
                }
                ui.add_space(8.0);
                if ui
                    .add_enabled(self.recover.is_none(), egui::Button::new(t.recover_one))
                    .clicked()
                {
                    self.begin_recover(vec![f.id]);
                }
            });
    }

    fn modals(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let lang = self.lang;
        if let Some((dir, ids)) = self.confirm_same_disk.clone() {
            let mut close = false;
            let mut go = false;
            let mut anyway = false;
            egui::Modal::new(Id::unique("same_disk")).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(RichText::new("⚠").size(28.0).color(WARN));
                ui.label(t.same_disk);
                ui.label(RichText::new(dir.display().to_string()).monospace().small());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t.choose_out).clicked() {
                        close = true;
                        go = true;
                    }
                    if ui.button(t.cancel).clicked() {
                        close = true;
                    }
                    if ui
                        .button(RichText::new(t.continue_anyway).color(BAD))
                        .clicked()
                    {
                        close = true;
                        anyway = true;
                    }
                });
            });
            if close {
                self.confirm_same_disk = None;
                if anyway {
                    self.run_recover(dir, ids);
                } else if go && self.pick_out_dir() {
                    self.begin_recover(ids);
                }
            }
        }

        let mut close_job = false;
        if let Some(j) = &self.recover {
            egui::Modal::new(Id::unique("recover")).show(ctx, |ui| {
                ui.set_min_width(420.0);
                match &j.report {
                    None => {
                        let p = &j.progress;
                        use std::sync::atomic::Ordering::Relaxed;
                        let (fd, ft) = (p.files_done.load(Relaxed), p.files_total.load(Relaxed));
                        let (bd, bt) = (p.bytes_done.load(Relaxed), p.bytes_total.load(Relaxed));
                        ui.heading(t.recovering);
                        ui.add_space(6.0);
                        let frac = if bt > 0 { bd as f32 / bt as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(frac).fill(ACCENT).show_percentage());
                        let el = j.started.elapsed().as_secs_f64().max(0.001);
                        ui.label(format!(
                            "{} / {} · {} / {} · {}/s",
                            fmt_num(fd, lang),
                            fmt_num(ft, lang),
                            fmt_size(bd),
                            fmt_size(bt),
                            fmt_size((bd as f64 / el) as u64)
                        ));
                        if ui.button(t.cancel).clicked() {
                            p.cancel.store(true, Relaxed);
                        }
                    }
                    Some(r) => {
                        ui.label(RichText::new("✔").size(30.0).color(GOOD));
                        ui.heading(format!("{} {}", fmt_num(r.written, lang), t.recovered));
                        ui.label(format!("{} · {}", fmt_size(r.bytes), j.out.display()));
                        if !r.failed.is_empty() {
                            ui.colored_label(BAD, format!("{} {}", r.failed.len(), t.failed));
                            egui::ScrollArea::vertical()
                                .max_height(120.0)
                                .show(ui, |ui| {
                                    for (n, e) in r.failed.iter().take(200) {
                                        ui.label(RichText::new(format!("{n}: {e}")).small());
                                    }
                                });
                        }
                        if r.bad_sectors > 0 {
                            ui.colored_label(WARN, format!("{}: {}", t.bad_sectors, r.bad_sectors));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button(t.open_folder).clicked() {
                                open_folder(&j.out);
                            }
                            if ui.button(t.close).clicked() {
                                close_job = true;
                            }
                        });
                    }
                }
            });
        }
        if close_job {
            self.recover = None;
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            if self.screen == Screen::Source {
                let p = p.to_string_lossy().into_owned();
                self.path_input = p.clone();
                self.selected_path = Some(p);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.handle_drops(&ctx);
        self.top_bar(ui);
        match self.screen {
            Screen::Source => self.source_screen(ui),
            Screen::Results => self.results_screen(ui),
        }
        self.modals(&ctx);
        if self.scan.is_some() || self.recover.as_ref().is_some_and(|j| j.report.is_none()) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

// ------------------------------------------------------------------ helpers

/// A clickable option card; returns true when clicked anywhere on it.
fn mode_card(ui: &mut egui::Ui, selected: bool, title: &str, hint: &str) -> bool {
    let stroke = if selected {
        egui::Stroke::new(2.0, ACCENT)
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let fill = if selected {
        ACCENT.gamma_multiply(0.12)
    } else {
        ui.visuals().faint_bg_color
    };
    let mut radio_clicked = false;
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(64.0);
            ui.vertical(|ui| {
                radio_clicked = ui.radio(selected, RichText::new(title).strong()).clicked();
                ui.label(RichText::new(hint).small().color(MUTED));
            });
        })
        .response;
    r.interact(Sense::click()).clicked() || radio_clicked
}

fn stat(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(RichText::new(k).small().color(MUTED));
    ui.label(RichText::new(v).strong());
    ui.add_space(10.0);
}

pub(crate) fn cat_icon(c: Category) -> &'static str {
    match c {
        Category::Image => "🖼",
        Category::Video => "🎞",
        Category::Audio => "🎵",
        Category::Document => "📄",
        Category::Archive => "📦",
        Category::Other => "⚙",
    }
}

fn cond_style(c: Condition, t: &T) -> (Color32, String) {
    let (color, icon) = match c {
        Condition::Good => (GOOD, "✔"),
        Condition::Unknown => (MUTED, "？"),
        Condition::Partial => (WARN, "⚠"),
        Condition::Overwritten => (BAD, "✖"),
    };
    (color, format!("{icon} {}", t.cond[c as usize]))
}

fn origin_label(o: Origin, t: &T) -> &'static str {
    match o {
        Origin::Ntfs => "NTFS",
        Origin::Fat => "FAT",
        Origin::ExFat => "exFAT",
        Origin::Carved => t.origin_carved,
    }
}

fn fmt_num(n: usize, lang: Lang) -> String {
    let sep = if lang == Lang::Id { '.' } else { ',' };
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn icmp(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

fn open_folder(p: &std::path::Path) {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

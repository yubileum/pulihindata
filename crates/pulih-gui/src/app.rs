//! Application state, the navigation sidebar, and shared actions. The three
//! pages live in `home.rs`, `results.rs` and the tips page below; modal
//! dialogs in `dialogs.rs`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver};
use eframe::egui::{self, Color32, RichText, Sense, TextureOptions, ThemePreference, Vec2};
use pulih_core::{
    canon_ext, list_devices, recover, same_disk, start_scan, Category, Condition, Device,
    DeviceInfo, FoundFile, Inspection, Origin, ReadAt, RecoverOptions, RecoverProgress,
    RecoverReport, ScanEvent, ScanHandle, ScanOptions, ScanSummary, Volume,
};

use crate::find::{self, Find};
use crate::i18n::{Lang, T};
use crate::preview::{Preview, Previewer};
use crate::theme::{self, bold, pal, semi, DriveIcon, NavIcon};
use crate::thumbs::Thumbs;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Results,
    Tips,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Both,
    Quick,
    Deep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Name,
    Folder,
    Type,
    Size,
    Modified,
    Condition,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OriginFilter {
    All,
    Fs,
    Carved,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Grid,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavTab {
    Path,
    Type,
}

/// A place the user can scan.
#[derive(Clone, PartialEq)]
pub struct Loc {
    /// Identity for "is this card selected".
    pub key: String,
    pub path: String,
    pub title: String,
    pub sub: String,
    pub icon: DriveIcon,
    /// Only show results under this folder (scanning e.g. "Desktop").
    pub folder: Option<String>,
}

impl Loc {
    pub fn image(p: &std::path::Path) -> Loc {
        let s = p.to_string_lossy().into_owned();
        let title = p
            .file_name()
            .map_or(s.clone(), |n| n.to_string_lossy().into_owned());
        Loc {
            key: s.clone(),
            path: s.clone(),
            title,
            sub: s,
            icon: DriveIcon::Image,
            folder: None,
        }
    }
}

pub enum PreviewState {
    Loading,
    Image(egui::TextureHandle, (u32, u32)),
    Text(String),
    Hex(String),
}

pub struct RecoverJob {
    pub progress: Arc<RecoverProgress>,
    pub rx: Receiver<RecoverReport>,
    pub report: Option<RecoverReport>,
    pub out: PathBuf,
    pub started: Instant,
}

/// The "Recover" dialog before it runs.
pub struct RecoverSetup {
    pub ids: Vec<u64>,
    pub bytes: u64,
    pub same_disk: bool,
    pub accept_risk: bool,
    /// `out_dir` that `same_disk` was computed for.
    pub checked_dir: String,
}

/// Result counts per category, then per extension (for the Type tab).
pub type TypeGroups = Vec<(Category, usize, Vec<(String, usize)>)>;

pub struct App {
    pub lang: Lang,
    pub page: Page,
    pub logo: Option<egui::TextureHandle>,

    // Home
    pub devices: Vec<DeviceInfo>,
    pub places: Vec<(usize, PathBuf)>,
    pub sel: Option<Loc>,
    pub path_input: String,
    pub mode: Mode,
    pub opts: ScanOptions,
    pub show_more: bool,
    pub error: Option<String>,

    // Scan session
    pub source: Option<Arc<Device>>,
    pub scanned: Option<Loc>,
    pub scan: Option<ScanHandle>,
    pub vols: Vec<Volume>,
    pub summary: Option<ScanSummary>,
    pub logs: Vec<String>,

    // Results
    pub files: Vec<FoundFile>,
    pub checked: HashSet<u64>,
    pub checked_bytes: u64,
    pub view: Vec<u32>,
    pub view_dirty: bool,
    pub last_build: Instant,
    pub find: Find,
    pub good_only: bool,
    pub origin: OriginFilter,
    pub ext_counts: HashMap<String, usize>,
    pub types: TypeGroups,
    pub carved_exts: Vec<(String, usize)>,
    pub fs_count: usize,
    pub carved_count: usize,
    pub tree: Vec<(u16, String, find::Node)>,
    pub tree_sel: Option<(u16, String)>,
    pub stats_dirty: bool,
    pub deep_limit: Option<Vec<String>>,
    pub sort: (Col, bool),
    pub focus: Option<u64>,
    pub scroll_to: Option<usize>,
    pub view_mode: ViewMode,
    pub nav_tab: NavTab,
    pub show_preview: bool,
    pub grid_cols: usize,
    pub grid_rows: std::ops::Range<usize>,

    pub previewer: Previewer,
    pub preview: Option<(u64, PreviewState)>,
    pub inspection: Option<(u64, Inspection)>,
    pub thumbs: Thumbs,

    // Recovery
    pub setup: Option<RecoverSetup>,
    pub recover: Option<RecoverJob>,
    pub keep_folders: bool,
    pub out_dir: String,
}

pub fn known_places() -> Vec<(usize, PathBuf)> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let Some(home) = home else { return Vec::new() };
    [
        "Desktop",
        "Documents",
        "Pictures",
        "Downloads",
        "Videos",
        "Music",
    ]
    .iter()
    .enumerate()
    .map(|(i, n)| (i, home.join(n)))
    .filter(|(_, p)| p.is_dir())
    .collect()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let lang = match std::env::var("LANG")
            .unwrap_or_default()
            .to_ascii_lowercase()
        {
            l if l.starts_with("en") => Lang::En,
            _ => Lang::Id,
        };
        let icon = theme::icon();
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            ),
            TextureOptions::LINEAR,
        );
        // `pulih <image or device>` preselects it (e.g. "Open with").
        let sel = std::env::args_os()
            .nth(1)
            .map(|a| Loc::image(std::path::Path::new(&a)));
        App {
            lang,
            page: Page::Home,
            logo: Some(logo),
            devices: list_devices(),
            places: known_places(),
            sel,
            path_input: String::new(),
            mode: Mode::Both,
            opts: ScanOptions::default(),
            show_more: false,
            error: None,
            source: None,
            scanned: None,
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
            find: Find::default(),
            good_only: false,
            origin: OriginFilter::All,
            ext_counts: HashMap::new(),
            types: Vec::new(),
            carved_exts: Vec::new(),
            fs_count: 0,
            carved_count: 0,
            tree: Vec::new(),
            tree_sel: None,
            stats_dirty: false,
            deep_limit: None,
            sort: (Col::Condition, true),
            focus: None,
            scroll_to: None,
            view_mode: ViewMode::List,
            nav_tab: NavTab::Path,
            show_preview: true,
            grid_cols: 1,
            grid_rows: 0..0,
            previewer: Previewer::default(),
            preview: None,
            inspection: None,
            thumbs: Thumbs::default(),
            setup: None,
            recover: None,
            keep_folders: true,
            out_dir: String::new(),
        }
    }

    pub fn t(&self) -> &'static T {
        self.lang.t()
    }

    pub fn scanning(&self) -> bool {
        self.scan.is_some() && self.summary.is_none()
    }

    pub fn has_session(&self) -> bool {
        self.source.is_some()
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
                        self.files.extend(v);
                        changed = true;
                    }
                    ScanEvent::CarveFinal(v) => {
                        self.files.retain(|f| f.origin != Origin::Carved);
                        self.files.extend(v);
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

    pub fn recount_checked(&mut self) {
        self.checked_bytes = self
            .files
            .iter()
            .filter(|f| self.checked.contains(&f.id))
            .map(|f| f.size)
            .sum();
    }

    pub fn toggle_check(&mut self, id: u64) {
        if !self.checked.remove(&id) {
            self.checked.insert(id);
        }
        self.recount_checked();
    }

    pub fn rebuild_view(&mut self) {
        if self.stats_dirty {
            self.ext_counts = find::ext_counts(&self.files);
            let mut carved: HashMap<String, usize> = HashMap::new();
            self.fs_count = 0;
            for f in &self.files {
                if f.origin == Origin::Carved {
                    *carved.entry(canon_ext(&f.ext)).or_insert(0) += 1;
                } else {
                    self.fs_count += 1;
                }
            }
            self.carved_count = self.files.len() - self.fs_count;
            self.carved_exts = by_count(carved);
            self.types = Category::ALL
                .iter()
                .map(|&c| {
                    let exts: HashMap<String, usize> = self
                        .ext_counts
                        .iter()
                        .filter(|(e, _)| Category::from_ext(e) == c)
                        .map(|(e, n)| (e.clone(), *n))
                        .collect();
                    (c, exts.values().sum(), by_count(exts))
                })
                .filter(|g| g.1 > 0)
                .collect();
            self.tree = find::build_tree(&self.files, &self.vols);
            self.stats_dirty = false;
        }
        let target = self.find.target();
        let (good_only, origin, sel) = (self.good_only, self.origin, &self.tree_sel);
        let mut view: Vec<u32> = (0..self.files.len() as u32)
            .filter(|&i| {
                let f = &self.files[i as usize];
                (!good_only || f.condition == Condition::Good)
                    && match origin {
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

    pub fn reset_filters(&mut self) {
        self.find = Find::default();
        self.tree_sel = None;
        self.good_only = false;
        self.origin = OriginFilter::All;
        self.view_dirty = true;
    }

    pub fn filters_active(&self) -> bool {
        self.find.is_active()
            || self.tree_sel.is_some()
            || self.good_only
            || self.origin != OriginFilter::All
    }

    pub fn start_scan(&mut self) {
        let t = self.t();
        let loc = match self.sel.clone() {
            Some(l) => l,
            None if !self.path_input.trim().is_empty() => {
                let p = self.path_input.trim().to_string();
                Loc {
                    key: p.clone(),
                    title: p.clone(),
                    sub: String::new(),
                    path: p,
                    icon: DriveIcon::Image,
                    folder: None,
                }
            }
            None => {
                self.error = Some(t.pick_source_first.into());
                return;
            }
        };
        let dev = match Device::open(&loc.path) {
            Ok(d) => Arc::new(d),
            Err(e) => {
                self.error = Some(if e.kind() == std::io::ErrorKind::PermissionDenied {
                    t.need_admin.to_string()
                } else {
                    format!("{} {}: {e}", t.open_failed, loc.path)
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
        self.error = None;
        self.files.clear();
        self.checked.clear();
        self.checked_bytes = 0;
        self.view.clear();
        self.logs.clear();
        self.summary = None;
        self.vols.clear();
        self.focus = None;
        self.preview = None;
        self.inspection = None;
        self.tree.clear();
        self.ext_counts.clear();
        self.types.clear();
        self.carved_exts.clear();
        self.fs_count = 0;
        self.carved_count = 0;
        self.thumbs.clear();
        // Scanning a folder location = scanning its volume, showing that folder.
        self.tree_sel = loc.folder.clone().map(|f| (0, f));
        self.scan = Some(start_scan(dev.clone(), o));
        self.source = Some(dev);
        self.scanned = Some(loc);
        self.page = Page::Results;
    }

    pub fn set_focus(&mut self, idx: usize, ctx: &egui::Context) {
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

    pub fn pick_out_dir(&mut self) -> bool {
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

    pub fn open_recover(&mut self, ids: Vec<u64>) {
        if ids.is_empty() || self.recover.is_some() {
            return;
        }
        let want: HashSet<u64> = ids.iter().copied().collect();
        let bytes = self
            .files
            .iter()
            .filter(|f| want.contains(&f.id))
            .map(|f| f.size)
            .sum();
        self.setup = Some(RecoverSetup {
            ids,
            bytes,
            same_disk: false,
            accept_risk: false,
            checked_dir: String::new(),
        });
        self.check_same_disk();
    }

    /// Re-check the destination against the source disk when it changes.
    pub fn check_same_disk(&mut self) {
        let src = self
            .source
            .as_ref()
            .map(|s| s.path.clone())
            .unwrap_or_default();
        let dir = self.out_dir.trim().to_string();
        if let Some(s) = &mut self.setup {
            if s.checked_dir != dir {
                s.same_disk = !dir.is_empty() && same_disk(&src, std::path::Path::new(&dir));
                s.accept_risk = false;
                s.checked_dir = dir;
            }
        }
    }

    pub fn run_recover(&mut self, dir: PathBuf, ids: Vec<u64>) {
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
            self.sel = Some(Loc::image(&p));
            self.page = Page::Home;
        }
    }

    // --------------------------------------------------------------- sidebar

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        egui::Panel::left("sidebar")
            .exact_size(208.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(egui::Margin::symmetric(14, 18)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(l) = &self.logo {
                        ui.add(egui::Image::new((l.id(), Vec2::splat(38.0))).corner_radius(9));
                    }
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(bold("Pulih").size(22.0).color(p.text));
                        ui.label(RichText::new(t.tagline).size(11.5).color(p.muted));
                    });
                });
                ui.add_space(22.0);
                let n = self.files.len();
                let items = [
                    (Page::Home, NavIcon::Home, t.nav_home, None, true),
                    (
                        Page::Results,
                        NavIcon::Results,
                        t.nav_results,
                        (n > 0).then_some(n),
                        self.has_session(),
                    ),
                    (Page::Tips, NavIcon::Tips, t.nav_tips, None, true),
                ];
                for (page, icon, label, badge, enabled) in items {
                    if side_item(ui, icon, label, badge, self.page == page, enabled).clicked() {
                        self.page = page;
                    }
                    ui.add_space(4.0);
                }
                if self.scanning() && self.page != Page::Results {
                    ui.add_space(8.0);
                    if let Some(h) = &self.scan {
                        let s = h.progress.snapshot();
                        ui.label(
                            RichText::new(format!(
                                "{}… {:.0}%",
                                t.scanning_title,
                                s.fraction() * 100.0
                            ))
                            .size(12.0)
                            .color(p.muted),
                        );
                        theme::progress(ui, s.fraction(), 6.0, true);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(11.0)
                            .color(p.muted),
                    );
                    ui.add_space(6.0);
                    let cur = match ui.ctx().options(|o| o.theme_preference) {
                        ThemePreference::Light => 0,
                        ThemePreference::Dark => 1,
                        ThemePreference::System => 2,
                    };
                    if let Some(i) = theme::segmented_fill(ui, &t.theme_names, cur) {
                        ui.ctx().set_theme(
                            [
                                ThemePreference::Light,
                                ThemePreference::Dark,
                                ThemePreference::System,
                            ][i],
                        );
                    }
                    ui.label(semi(t.appearance).size(12.0).color(p.muted));
                    ui.add_space(8.0);
                    let li = if self.lang == Lang::Id { 0 } else { 1 };
                    if let Some(i) = theme::segmented_fill(ui, &["Indonesia", "English"], li) {
                        self.lang = if i == 0 { Lang::Id } else { Lang::En };
                    }
                    ui.label(semi(t.language).size(12.0).color(p.muted));
                });
            });
    }

    // ------------------------------------------------------------------ tips

    fn tips(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(32, 28)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        theme::title(ui, t.tips_title, Some(t.tips_sub));
                        ui.add_space(18.0);
                        let cols = if ui.available_width() > 760.0 { 2 } else { 1 };
                        let w = (ui.available_width() - 16.0 * (cols as f32 - 1.0)) / cols as f32;
                        for row in (0..6).collect::<Vec<_>>().chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                for &i in row {
                                    theme::card(ui).show(ui, |ui| {
                                        ui.set_width(w - 34.0);
                                        ui.set_min_height(120.0);
                                        ui.horizontal_top(|ui| {
                                            let (r, _) = ui.allocate_exact_size(
                                                Vec2::splat(32.0),
                                                Sense::hover(),
                                            );
                                            ui.painter().circle_filled(
                                                r.center(),
                                                16.0,
                                                p.primary_soft,
                                            );
                                            ui.painter().text(
                                                r.center(),
                                                egui::Align2::CENTER_CENTER,
                                                (i + 1).to_string(),
                                                theme::font_bold(14.0),
                                                p.primary,
                                            );
                                            ui.add_space(6.0);
                                            ui.vertical(|ui| {
                                                ui.add_space(5.0);
                                                ui.label(
                                                    bold(t.tip_titles[i]).size(15.0).color(p.text),
                                                );
                                                ui.add_space(2.0);
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(t.tip_bodies[i])
                                                            .color(p.muted),
                                                    )
                                                    .wrap(),
                                                );
                                            });
                                        });
                                    });
                                }
                            });
                            ui.add_space(16.0);
                        }
                    });
            });
    }
}

/// Sidebar navigation item.
fn side_item(
    ui: &mut egui::Ui,
    icon: NavIcon,
    label: &str,
    badge: Option<usize>,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 40.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 10, p.primary_soft);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 9.0), Vec2::new(3.0, 22.0)),
            2,
            p.primary,
        );
    } else if resp.hovered() && enabled {
        painter.rect_filled(rect, 10, p.faint);
    }
    let fg = if !enabled {
        p.muted.gamma_multiply(0.6)
    } else if selected {
        p.primary
    } else {
        p.text
    };
    theme::paint_nav_icon(painter, rect.left_center() + Vec2::new(22.0, 0.0), icon, fg);
    painter.text(
        rect.left_center() + Vec2::new(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(
            14.0,
            egui::FontFamily::Name(if selected { "bold" } else { "semibold" }.into()),
        ),
        fg,
    );
    if let Some(n) = badge {
        let txt = fmt_count(n);
        let g = painter.layout_no_wrap(
            txt,
            egui::FontId::new(11.0, egui::FontFamily::Name("bold".into())),
            Color32::WHITE,
        );
        let size = Vec2::new(g.size().x + 12.0, 18.0);
        let br = egui::Rect::from_center_size(
            rect.right_center() - Vec2::new(size.x / 2.0 + 10.0, 0.0),
            size,
        );
        painter.rect_filled(br, 9, p.primary);
        painter.galley(br.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn fmt_count(n: usize) -> String {
    if n >= 10_000 {
        format!("{}k", n / 1000)
    } else {
        n.to_string()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        self.thumbs.pump(&ctx);
        self.handle_drops(&ctx);
        self.sidebar(ui);
        match self.page {
            Page::Home => self.home(ui),
            Page::Results => self.results(ui),
            Page::Tips => self.tips(ui),
        }
        self.dialogs(&ctx);
        if self.scan.is_some() || self.recover.as_ref().is_some_and(|j| j.report.is_none()) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

// ------------------------------------------------------------------ helpers

pub fn fmt_num(n: usize, lang: Lang) -> String {
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

pub fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn icmp(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}

pub fn origin_label(o: Origin, t: &T) -> &'static str {
    match o {
        Origin::Ntfs => "NTFS",
        Origin::Fat => "FAT",
        Origin::ExFat => "exFAT",
        Origin::Carved => t.origin_carved,
    }
}

pub fn open_folder(p: &std::path::Path) {
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(p).spawn();
}

/// `(key, count)` pairs, most frequent first.
fn by_count(m: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = m.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

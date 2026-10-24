//! Results page: scan header, Path/Type navigation, filter toolbar, list and
//! thumbnail views, the preview pane, and the Recover bar.

use std::collections::BTreeSet;

use eframe::egui::{
    self, Align, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, Vec2,
};
use egui_extras::{Column, TableBuilder};
use pulih_core::bytes::{fmt_size, fmt_time};
use pulih_core::{Category, FileData, FoundFile, Inspection, Origin, Phase};

use crate::app::{
    fmt_dur, fmt_num, origin_label, App, Col, NavTab, OriginFilter, Page, PreviewState, ViewMode,
};
use crate::find::{self, DatePreset, Mini};
use crate::home::paint_magnifier;
use crate::i18n::T;
use crate::theme::{self, bold, pal, semi, Pal};

fn date_only(t: i64) -> String {
    let s = fmt_time(t);
    s.get(..10).unwrap_or(&s).to_string()
}

/// Search field with a magnifier and a clear button.
fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str, w: f32) -> bool {
    let p = pal(ui);
    let mut changed = false;
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(8)
        .inner_margin(egui::Margin {
            left: 10,
            right: 6,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                paint_magnifier(ui.painter(), r.center(), 7.0, p.muted);
                let te = egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .frame(egui::Frame::NONE)
                    .desired_width(w);
                changed |= ui.add(te).changed();
                if !text.is_empty() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                    let c = if resp.hovered() { p.text } else { p.muted };
                    theme::paint_x(ui.painter(), r.center(), 4.0, c);
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        text.clear();
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Removable chip for an active filter; true when clicked.
fn filter_chip(ui: &mut egui::Ui, text: &str) -> bool {
    let p = pal(ui);
    let resp = ui
        .add(
            egui::Button::new(semi(format!("{text}      ")).size(12.0).color(p.primary))
                .fill(p.primary_soft)
                .stroke(Stroke::NONE)
                .corner_radius(14)
                .min_size(Vec2::new(0.0, 24.0)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    theme::paint_x(
        ui.painter(),
        resp.rect.right_center() - Vec2::new(13.0, 0.0),
        3.5,
        p.primary,
    );
    resp.clicked()
}

fn short(s: String, n: usize) -> String {
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// One thumbnail tile; returns (tile response, checkbox clicked).
#[allow(clippy::too_many_arguments)]
fn tile_ui(
    ui: &mut egui::Ui,
    f: &FoundFile,
    size: Vec2,
    checked: bool,
    focused: bool,
    thumb: Option<Option<TextureHandle>>,
    p: &Pal,
    t: &T,
) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect.expand(2.0));
    let hovered = resp.hovered();
    let stroke = if focused {
        Stroke::new(2.0, p.primary)
    } else if hovered {
        Stroke::new(1.0, p.primary.gamma_multiply(0.6))
    } else {
        Stroke::new(1.0, p.border)
    };
    painter.rect(
        rect,
        10,
        if checked { p.primary_soft } else { p.card },
        stroke,
        StrokeKind::Inside,
    );
    let aw = size.x - 16.0;
    let area = Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::new(aw, aw * 0.78));
    painter.rect_filled(area, 8, p.faint);
    match thumb {
        Some(Some(tex)) => {
            // Fill the frame, cropping the overflow (like a photo gallery).
            let ts = tex.size_vec2();
            let scale = (area.width() / ts.x).max(area.height() / ts.y);
            let vis = Vec2::new(
                area.width() / (ts.x * scale),
                area.height() / (ts.y * scale),
            );
            let uv = Rect::from_center_size(Pos2::new(0.5, 0.5), vis);
            egui::Image::new((tex.id(), area.size()))
                .uv(uv)
                .corner_radius(8)
                .paint_at(ui, area);
        }
        _ => {
            let icon = Rect::from_center_size(
                area.center(),
                Vec2::new(area.width() * 0.5, area.height() * 0.62),
            );
            theme::paint_file(&painter, icon, &f.ext, f.category, p);
        }
    }
    let cb = Rect::from_min_size(area.min + Vec2::splat(7.0), Vec2::splat(18.0));
    let cb_resp = ui.interact(cb, resp.id.with("cb"), Sense::click());
    if checked {
        painter.rect_filled(cb, 5, p.primary);
        let s = Stroke::new(2.0, egui::Color32::WHITE);
        let (a, b, c) = (
            cb.center() + Vec2::new(-4.5, 0.0),
            cb.center() + Vec2::new(-1.0, 3.5),
            cb.center() + Vec2::new(5.0, -3.5),
        );
        painter.line_segment([a, b], s);
        painter.line_segment([b, c], s);
    } else {
        let border = if hovered || cb_resp.hovered() {
            p.primary
        } else {
            p.muted.gamma_multiply(0.7)
        };
        painter.rect(cb, 5, p.card, Stroke::new(1.3, border), StrokeKind::Inside);
    }
    let (cc, _) = theme::cond_colors(p, f.condition);
    let dot = Pos2::new(area.right() - 12.0, area.top() + 12.0);
    painter.circle_filled(dot, 6.5, p.card);
    painter.circle_filled(dot, 4.5, cc);
    let x = rect.left() + 9.0;
    let name = theme::elided(&painter, &f.name, theme::font_semi(12.5), p.text, aw);
    painter.galley(Pos2::new(x, area.bottom() + 7.0), name, p.text);
    let meta = match f.modified {
        Some(m) => format!("{}  ·  {}", fmt_size(f.size), date_only(m)),
        None => format!("{}  ·  {}", fmt_size(f.size), f.ext.to_uppercase()),
    };
    let meta = theme::elided(
        &painter,
        &meta,
        egui::FontId::proportional(11.0),
        p.muted,
        aw,
    );
    painter.galley(Pos2::new(x, area.bottom() + 26.0), meta, p.muted);
    let resp = resp.on_hover_text(format!(
        "{}\n{}",
        f.full_path(),
        t.cond[f.condition as usize]
    ));
    (resp, cb_resp.clicked())
}

impl App {
    pub fn results(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let ctx = ui.ctx().clone();
        self.scan_header(ui);
        self.recover_bar(ui);
        // Too narrow for three columns: the preview pane steps aside.
        let roomy = ui.available_width() >= 940.0;
        self.nav_panel(ui);
        if self.show_preview && roomy {
            self.preview_panel(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 12,
                bottom: 12,
            }))
            .show(ui, |ui| {
                if self.toolbar(ui, roomy) {
                    self.view_dirty = true;
                }
                ui.add_space(8.0);
                self.keyboard(&ctx);
                theme::card(ui)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        if self.view.is_empty() {
                            self.thumbs.set_visible([]);
                            self.empty_state(ui);
                        } else if self.view_mode == ViewMode::List {
                            self.thumbs.set_visible([]);
                            self.list(ui, &ctx);
                        } else {
                            self.grid(ui, &ctx);
                        }
                    });
            });
    }

    // ---------------------------------------------------------------- header

    fn scan_header(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        let lang = self.lang;
        let mut new_scan = false;
        egui::Panel::top("scanhead")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 14,
                bottom: 0,
            }))
            .show(ui, |ui| {
                theme::card(ui)
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let loc = self.scanned.clone();
                        let snap = self.scan.as_ref().map(|h| h.progress.snapshot());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                            theme::paint_drive(
                                ui.painter(),
                                r,
                                loc.as_ref().map_or(theme::DriveIcon::Disk, |l| l.icon),
                                p,
                            );
                            ui.add_space(4.0);
                            let name = loc.as_ref().map_or(String::new(), |l| l.title.clone());
                            ui.vertical(|ui| {
                                ui.add_space(2.0);
                                let (title, sub) = match (&snap, &self.summary) {
                                    (Some(s), None) => {
                                        let phase = match s.phase {
                                            Phase::Analyzing | Phase::Idle => t.phase_analyzing,
                                            Phase::Filesystem => t.phase_fs,
                                            Phase::Carving => t.phase_carving,
                                            Phase::Done => t.phase_done,
                                            Phase::Cancelled => t.phase_cancelled,
                                        };
                                        let sub = if s.status.is_empty() {
                                            phase.to_string()
                                        } else {
                                            format!("{phase}  ·  {}", s.status)
                                        };
                                        (format!("{}: {name}", t.scanning_title), sub)
                                    }
                                    (_, Some(s)) => {
                                        let head = if s.cancelled {
                                            t.phase_cancelled
                                        } else {
                                            t.done_title
                                        };
                                        let sub = format!(
                                            "{} {}  ·  {} {}",
                                            fmt_num(self.files.len(), lang),
                                            t.files_found,
                                            t.elapsed,
                                            fmt_dur(s.elapsed)
                                        );
                                        (format!("{head} — {name}"), sub)
                                    }
                                    _ => (name, String::new()),
                                };
                                ui.label(bold(title).size(17.0).color(p.text));
                                if !sub.is_empty() {
                                    let w = ui.available_width() - 170.0;
                                    let g = theme::elided(
                                        ui.painter(),
                                        &sub,
                                        egui::FontId::proportional(12.5),
                                        p.muted,
                                        w,
                                    );
                                    ui.label(g);
                                }
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.scanning() {
                                    let b = egui::Button::new(semi(t.stop).color(p.danger))
                                        .fill(p.danger_soft)
                                        .stroke(Stroke::NONE)
                                        .min_size(Vec2::new(0.0, 34.0));
                                    if ui.add(b).clicked() {
                                        if let Some(h) = &self.scan {
                                            h.cancel();
                                        }
                                    }
                                } else if theme::soft_button(ui, t.new_scan).clicked() {
                                    new_scan = true;
                                }
                                if !self.logs.is_empty() {
                                    theme::dropdown(
                                        ui,
                                        &format!("{} ({})", t.log, self.logs.len()),
                                        false,
                                        |ui| {
                                            ui.set_width(460.0);
                                            egui::ScrollArea::vertical().max_height(320.0).show(
                                                ui,
                                                |ui| {
                                                    for l in &self.logs {
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(l)
                                                                    .size(12.0)
                                                                    .color(p.muted),
                                                            )
                                                            .wrap(),
                                                        );
                                                    }
                                                },
                                            );
                                        },
                                    );
                                }
                            });
                        });
                        if let (Some(s), true) = (&snap, self.scanning()) {
                            ui.add_space(10.0);
                            theme::progress(ui, s.fraction(), 8.0, true);
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 20.0;
                                let stat = |ui: &mut egui::Ui, k: &str, v: String| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 5.0;
                                        ui.label(RichText::new(k).size(12.0).color(p.muted));
                                        ui.label(semi(v).size(12.5).color(p.text));
                                    });
                                };
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    ui.label(
                                        bold(format!("{:.1}%", s.fraction() * 100.0))
                                            .size(13.0)
                                            .color(p.primary),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} / {}",
                                            fmt_size(s.done),
                                            fmt_size(s.total)
                                        ))
                                        .size(12.5)
                                        .color(p.muted),
                                    );
                                });
                                stat(ui, t.speed, format!("{}/s", fmt_size(s.rate() as u64)));
                                stat(ui, t.eta, s.eta().map_or("—".into(), fmt_dur));
                                stat(ui, t.elapsed, fmt_dur(s.elapsed));
                                stat(ui, t.found, fmt_num(s.found as usize, lang));
                                if s.bad_sectors > 0 {
                                    stat(ui, t.bad_sectors, fmt_num(s.bad_sectors as usize, lang));
                                }
                            });
                        } else if let Some(s) = &self.summary {
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                let (fg, bg) = if s.cancelled {
                                    (p.warning, p.warning_soft)
                                } else {
                                    (p.success, p.success_soft)
                                };
                                theme::pill(
                                    ui,
                                    if s.cancelled {
                                        t.phase_cancelled
                                    } else {
                                        t.phase_done
                                    },
                                    fg,
                                    bg,
                                );
                                theme::pill(
                                    ui,
                                    format!("{}: {}", t.origin_fs, fmt_num(s.fs_found, lang)),
                                    p.primary,
                                    p.primary_soft,
                                );
                                theme::pill(
                                    ui,
                                    format!("{}: {}", t.origin_carved, fmt_num(s.carved, lang)),
                                    p.primary,
                                    p.primary_soft,
                                );
                                if s.bad_sectors > 0 {
                                    theme::pill(
                                        ui,
                                        format!(
                                            "{}: {}",
                                            t.bad_sectors,
                                            fmt_num(s.bad_sectors as usize, lang)
                                        ),
                                        p.danger,
                                        p.danger_soft,
                                    );
                                }
                                if let Some(e) = &self.deep_limit {
                                    ui.label(
                                        RichText::new(format!(
                                            "🔎 {}: {}",
                                            t.deep_limited,
                                            e.join(", ").to_uppercase()
                                        ))
                                        .size(12.0)
                                        .color(p.muted),
                                    );
                                }
                            });
                        }
                    });
            });
        if new_scan {
            self.page = Page::Home;
            self.devices = pulih_core::list_devices();
        }
    }

    // ----------------------------------------------------------- navigation

    fn nav_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        let mut changed = false;
        egui::Panel::left("resnav")
            .resizable(true)
            .default_size(220.0)
            .size_range(180.0..=380.0)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ui, |ui| {
                let tab = if self.nav_tab == NavTab::Path { 0 } else { 1 };
                if let Some(i) = theme::segmented_fill(ui, &[t.tab_path, t.tab_type], tab) {
                    self.nav_tab = if i == 0 { NavTab::Path } else { NavTab::Type };
                }
                ui.add_space(10.0);
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        changed = match self.nav_tab {
                            NavTab::Path => self.path_tab(ui, t),
                            NavTab::Type => self.type_tab(ui, t),
                        };
                    });
            });
        if changed {
            self.view_dirty = true;
        }
    }

    fn path_tab(&mut self, ui: &mut egui::Ui, t: &'static T) -> bool {
        let mut changed = false;
        let plain = self.tree_sel.is_none();
        if find::nav_row(
            ui,
            Mini::Glyph("☰"),
            t.all_files,
            Some(self.files.len()),
            plain && self.origin == OriginFilter::All,
        )
        .clicked()
        {
            self.tree_sel = None;
            self.origin = OriginFilter::All;
            changed = true;
        }
        if self.fs_count > 0 {
            ui.add_space(6.0);
            if find::nav_row(
                ui,
                Mini::Glyph("🗑"),
                t.deleted_named,
                Some(self.fs_count),
                plain && self.origin == OriginFilter::Fs,
            )
            .clicked()
            {
                self.tree_sel = None;
                self.origin = OriginFilter::Fs;
                changed = true;
            }
            ui.indent("tree", |ui| {
                if find::tree_ui(ui, &self.tree, &mut self.tree_sel) {
                    self.origin = OriginFilter::All;
                    changed = true;
                }
            });
        }
        if self.carved_count > 0 {
            ui.add_space(6.0);
            let only = |exts: &BTreeSet<String>, e: &str| exts.len() == 1 && exts.contains(e);
            let sel = plain && self.origin == OriginFilter::Carved && self.find.exts.is_empty();
            if find::nav_row(
                ui,
                Mini::Search,
                t.found_by_sig,
                Some(self.carved_count),
                sel,
            )
            .clicked()
            {
                self.tree_sel = None;
                self.origin = OriginFilter::Carved;
                self.find.exts.clear();
                changed = true;
            }
            ui.indent("sig", |ui| {
                for (ext, n) in &self.carved_exts {
                    let on =
                        plain && self.origin == OriginFilter::Carved && only(&self.find.exts, ext);
                    let cat = Category::from_ext(ext);
                    if find::nav_row(
                        ui,
                        Mini::Dot(theme::type_color(ext, cat)),
                        &ext.to_uppercase(),
                        Some(*n),
                        on,
                    )
                    .clicked()
                    {
                        self.tree_sel = None;
                        self.origin = OriginFilter::Carved;
                        self.find.exts = BTreeSet::from([ext.clone()]);
                        changed = true;
                    }
                }
            });
        }
        changed
    }

    fn type_tab(&mut self, ui: &mut egui::Ui, t: &'static T) -> bool {
        let mut changed = false;
        if find::nav_row(
            ui,
            Mini::Glyph("☰"),
            t.all_files,
            Some(self.files.len()),
            self.find.exts.is_empty(),
        )
        .clicked()
        {
            self.find.exts.clear();
            changed = true;
        }
        ui.add_space(6.0);
        for (cat, n, exts) in &self.types {
            let all: BTreeSet<String> = exts.iter().map(|e| e.0.clone()).collect();
            let id = ui.make_persistent_id(("typegroup", *cat as u8));
            let open_default = exts.iter().any(|e| self.find.exts.contains(&e.0));
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                open_default,
            )
            .show_header(ui, |ui| {
                if find::nav_row(
                    ui,
                    Mini::Glyph(find::cat_icon(*cat)),
                    t.cats[*cat as usize],
                    Some(*n),
                    self.find.exts == all,
                )
                .clicked()
                {
                    self.find.exts = all.clone();
                    changed = true;
                }
            })
            .body(|ui| {
                for (e, c) in exts {
                    let on = self.find.exts.len() == 1 && self.find.exts.contains(e);
                    if find::nav_row(
                        ui,
                        Mini::Dot(theme::type_color(e, *cat)),
                        &e.to_uppercase(),
                        Some(*c),
                        on,
                    )
                    .clicked()
                    {
                        self.find.exts = BTreeSet::from([e.clone()]);
                        changed = true;
                    }
                }
            });
        }
        changed
    }

    // -------------------------------------------------------------- toolbar

    fn toolbar(&mut self, ui: &mut egui::Ui, roomy: bool) -> bool {
        let t = self.t();
        let p = pal(ui);
        let lang = self.lang;
        let mut changed = false;
        // Row 1: search, view mode, preview toggle.
        ui.horizontal(|ui| {
            let w = (ui.available_width() - if roomy { 340.0 } else { 250.0 }).max(110.0);
            changed |= search_box(ui, &mut self.find.names, t.search_files, w);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let on = self.show_preview;
                let b = egui::Button::new(semi(t.preview).size(13.0).color(if on {
                    p.primary
                } else {
                    p.muted
                }))
                .fill(if on { p.primary_soft } else { p.card })
                .stroke(Stroke::new(1.0, if on { p.primary } else { p.border }))
                .min_size(Vec2::new(0.0, 32.0));
                if roomy && ui.add(b).clicked() {
                    self.show_preview = !on;
                }
                let cur = if self.view_mode == ViewMode::List {
                    0
                } else {
                    1
                };
                if let Some(i) = theme::segmented(ui, &[t.view_list, t.view_grid], cur) {
                    self.view_mode = if i == 0 {
                        ViewMode::List
                    } else {
                        ViewMode::Grid
                    };
                    if let Some(id) = self.focus {
                        self.scroll_to = self
                            .view
                            .iter()
                            .position(|&i| self.files[i as usize].id == id);
                    }
                }
            });
        });
        ui.add_space(6.0);
        // Row 2: filter dropdowns, extra chips, result count.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let f = &mut self.find;
            let label = |name: &str, v: Option<String>| match v {
                Some(v) => format!("{name}: {}", short(v, 16)),
                None => name.to_string(),
            };
            // A whole category picked in the Type tab reads as its name.
            let whole_cat = self.types.iter().find_map(|(c, _, exts)| {
                (exts.len() == f.exts.len() && exts.iter().all(|e| f.exts.contains(&e.0)))
                    .then(|| t.cats[*c as usize].to_string())
            });
            let ty = label(
                t.filter_type,
                (!f.exts.is_empty())
                    .then(|| whole_cat.unwrap_or_else(|| find::type_summary(&f.exts))),
            );
            theme::dropdown(ui, &ty, !f.exts.is_empty(), |ui| {
                ui.set_max_width(470.0);
                changed |= find::type_picker(ui, &mut f.exts, Some(&self.ext_counts), t);
            });
            let dt = label(
                t.filter_date,
                (f.date != DatePreset::Any).then(|| find::date_summary(f, t)),
            );
            theme::dropdown(ui, &dt, f.date != DatePreset::Any, |ui| {
                ui.set_max_width(420.0);
                changed |= find::date_picker(ui, f, t);
            });
            let sized = f.min_mb > 0.0 || f.max_mb > 0.0;
            let sz = label(t.filter_size, sized.then(|| find::size_summary(f)));
            theme::dropdown(ui, &sz, sized, |ui| {
                changed |= find::size_picker(ui, f, t);
            });
            let cond = match (self.good_only, self.origin) {
                (true, _) => format!("{}: {}", t.filter_cond, t.cond[0]),
                (false, OriginFilter::Fs) => t.origin_fs.to_string(),
                (false, OriginFilter::Carved) => t.origin_carved.to_string(),
                (false, OriginFilter::All) => t.filter_cond.to_string(),
            };
            theme::dropdown(
                ui,
                &cond,
                self.good_only || self.origin != OriginFilter::All,
                |ui| {
                    ui.set_min_width(220.0);
                    changed |= ui.checkbox(&mut self.good_only, t.only_good).changed();
                    ui.separator();
                    ui.label(semi(t.col_source).size(12.5).color(p.muted));
                    for (o, l) in [
                        (OriginFilter::All, t.origin_all),
                        (OriginFilter::Fs, t.origin_fs),
                        (OriginFilter::Carved, t.origin_carved),
                    ] {
                        changed |= ui.radio_value(&mut self.origin, o, l).changed();
                    }
                },
            );
            let f = &mut self.find;
            if !f.folder.trim().is_empty()
                && filter_chip(
                    ui,
                    &format!("{}: {}", t.col_folder, short(f.folder.trim().into(), 28)),
                )
            {
                f.folder.clear();
                changed = true;
            }
            if let Some((vol, path)) = self.tree_sel.clone() {
                let name = if path.is_empty() {
                    self.vols
                        .iter()
                        .find(|v| v.index == vol)
                        .map_or_else(|| format!("Volume {vol}"), |v| v.desc.clone())
                } else {
                    path.rsplit('/').next().unwrap_or(&path).to_string()
                };
                if filter_chip(ui, &format!("{}: {}", t.col_folder, short(name, 28))) {
                    self.tree_sel = None;
                    changed = true;
                }
            }
            if self.filters_active() {
                let (total, shown) = (self.files.len(), self.view.len());
                ui.label(
                    semi(format!(
                        "{} {} {}",
                        fmt_num(shown, lang),
                        t.of,
                        fmt_num(total, lang)
                    ))
                    .size(12.5)
                    .color(p.muted),
                );
                let clear = egui::Button::new(semi(t.clear_filters).size(12.5).color(p.danger))
                    .frame(false);
                if ui
                    .add(clear)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.reset_filters();
                    changed = true;
                }
            }
        });
        changed
    }

    // ------------------------------------------------------------ keyboard

    fn keyboard(&mut self, ctx: &egui::Context) {
        if self.setup.is_some()
            || self.recover.is_some()
            || ctx.egui_wants_keyboard_input()
            || self.view.is_empty()
        {
            return;
        }
        let grid = self.view_mode == ViewMode::Grid;
        let cols = if grid {
            self.grid_cols.max(1) as isize
        } else {
            1
        };
        let (up, down, left, right, space) = ctx.input(|i| {
            use egui::Key;
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Space),
            )
        });
        let step = match (up, down, grid && left, grid && right) {
            (true, ..) => -cols,
            (_, true, ..) => cols,
            (_, _, true, _) => -1,
            (.., true) => 1,
            _ => 0,
        };
        if step != 0 {
            let pos = self.focus.and_then(|id| {
                self.view
                    .iter()
                    .position(|&i| self.files[i as usize].id == id)
            });
            let n = self.view.len() as isize;
            let next = pos.map_or(0, |p| (p as isize + step).clamp(0, n - 1)) as usize;
            self.set_focus(self.view[next] as usize, ctx);
            self.scroll_to = Some(next);
        }
        if space {
            if let Some(id) = self.focus {
                self.toggle_check(id);
            }
        }
    }

    // ---------------------------------------------------------------- views

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        let scanning = self.scanning();
        let filtered = !self.files.is_empty();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.26).max(16.0));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(84.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 42.0, p.primary_soft);
            if scanning {
                egui::Spinner::new()
                    .size(34.0)
                    .color(p.primary)
                    .paint_at(ui, Rect::from_center_size(r.center(), Vec2::splat(34.0)));
            } else {
                paint_magnifier(ui.painter(), r.center(), 20.0, p.primary);
            }
            ui.add_space(14.0);
            let (head, sub) = if scanning {
                (t.empty_scanning, "")
            } else if filtered {
                (t.empty_results, t.nothing_matches)
            } else {
                (t.nothing_found, t.try_deep)
            };
            ui.label(bold(head).size(16.0).color(p.text));
            if !sub.is_empty() {
                ui.add_space(2.0);
                ui.set_max_width(440.0);
                ui.add(egui::Label::new(RichText::new(sub).color(p.muted)).wrap());
            }
            if filtered && self.filters_active() {
                ui.add_space(12.0);
                if theme::soft_button(ui, t.clear_filters).clicked() {
                    self.reset_filters();
                }
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.t();
        let p = pal(ui);
        let wide = ui.available_width() > 700.0;
        let dated = ui.available_width() > 490.0;
        let mut toggled: Option<u64> = None;
        let mut clicked: Option<usize> = None;
        let mut sort_by: Option<Col> = None;
        let mut check_all: Option<bool> = None;
        let mut recover_one: Option<u64> = None;
        let (files, view, checked, focus) = (&self.files, &self.view, &self.checked, self.focus);
        let (sort_col, asc) = self.sort;
        let all_checked = view
            .iter()
            .all(|&i| checked.contains(&files[i as usize].id));

        ui.style_mut().interaction.selectable_labels = false;
        let mut tb = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(30.0))
            .column(Column::remainder().at_least(150.0).clip(true));
        if dated {
            tb = tb.column(Column::initial(124.0).at_least(80.0).clip(true));
        }
        if wide {
            tb = tb.column(Column::initial(56.0).at_least(44.0).clip(true));
        }
        tb = tb
            .column(Column::initial(80.0).at_least(60.0))
            .column(Column::initial(96.0).at_least(76.0));
        if wide {
            tb = tb.column(Column::initial(170.0).at_least(80.0).clip(true));
        }
        if let Some(r) = self.scroll_to.take() {
            tb = tb.scroll_to_row(r, None);
        }
        let mut cols = vec![(Col::Name, t.col_name)];
        if dated {
            cols.push((Col::Modified, t.col_modified));
        }
        if wide {
            cols.push((Col::Type, t.col_type));
        }
        cols.extend([(Col::Size, t.col_size), (Col::Condition, t.col_condition)]);
        if wide {
            cols.push((Col::Folder, t.col_folder));
        }
        tb.header(34.0, |mut h| {
            h.col(|ui| {
                let mut a = all_checked;
                if ui.checkbox(&mut a, "").changed() {
                    check_all = Some(a);
                }
            });
            for (c, label) in &cols {
                h.col(|ui| {
                    let on = sort_col == *c;
                    let arrow = match (on, asc) {
                        (false, _) => "",
                        (true, true) => "  ⏶",
                        (true, false) => "  ⏷",
                    };
                    let txt = semi(format!("{label}{arrow}")).size(12.5).color(if on {
                        p.text
                    } else {
                        p.muted
                    });
                    if ui.add(egui::Button::new(txt).frame(false)).clicked() {
                        sort_by = Some(*c);
                    }
                });
            }
        })
        .body(|body| {
            body.rows(34.0, view.len(), |mut row| {
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
                    let (r, _) = ui.allocate_exact_size(Vec2::new(20.0, 24.0), Sense::hover());
                    theme::paint_file(ui.painter(), r, &f.ext, f.category, p);
                    ui.add_space(2.0);
                    ui.label(semi(&f.name).size(13.0).color(p.text));
                    if let Some(i) = &f.info {
                        ui.label(RichText::new(i).size(11.5).color(p.muted));
                    }
                });
                if dated {
                    row.col(|ui| {
                        ui.label(
                            RichText::new(f.modified.map_or("—".into(), fmt_time))
                                .size(12.5)
                                .color(p.muted),
                        );
                    });
                }
                if wide {
                    row.col(|ui| {
                        ui.label(
                            RichText::new(f.ext.to_ascii_uppercase())
                                .size(12.0)
                                .color(p.muted),
                        );
                    });
                }
                row.col(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(fmt_size(f.size)).size(12.5).color(p.text));
                    });
                });
                row.col(|ui| {
                    theme::cond_pill(ui, f.condition, t);
                });
                if wide {
                    row.col(|ui| {
                        let s = if f.origin == Origin::Carved {
                            origin_label(f.origin, t)
                        } else if f.dir.is_empty() {
                            "/"
                        } else {
                            &f.dir
                        };
                        ui.label(RichText::new(s).size(12.0).color(p.muted));
                    });
                }
                let resp = row.response();
                if resp.double_clicked() {
                    toggled = Some(f.id);
                } else if resp.clicked() {
                    clicked = Some(idx);
                }
                resp.context_menu(|ui| {
                    if ui.button(t.recover_one).clicked() {
                        recover_one = Some(f.id);
                    }
                });
            });
        });

        if let Some(id) = toggled {
            self.toggle_check(id);
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
        if let Some(id) = recover_one {
            self.open_recover(vec![id]);
        }
    }

    fn grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let t = self.t();
        let p = pal(ui);
        let gap = 12.0;
        let avail = ui.available_width() - 4.0;
        let cols = (((avail + gap) / (150.0 + gap)).floor() as usize).max(1);
        let tw = ((avail - gap * (cols as f32 - 1.0)) / cols as f32)
            .min(220.0)
            .floor();
        let tile = Vec2::new(tw, (tw - 16.0) * 0.78 + 60.0);
        self.grid_cols = cols;
        let rows = self.view.len().div_ceil(cols);
        let row_h = tile.y + gap;
        let mut sa = egui::ScrollArea::vertical().auto_shrink(false);
        if let Some(k) = self.scroll_to.take() {
            let r = k / cols;
            let vh = ui.available_height();
            let (first, last) = (self.grid_rows.start, self.grid_rows.end.saturating_sub(1));
            // Bring the focused row fully into view (rows at the edges are
            // usually only partly visible).
            if r <= first {
                sa = sa.vertical_scroll_offset(r as f32 * row_h);
            } else if r >= last {
                sa = sa.vertical_scroll_offset(((r + 1) as f32 * row_h - vh).max(0.0));
            }
        }
        let mut clicked: Option<usize> = None;
        let mut toggled: Option<u64> = None;
        let mut recover_one: Option<u64> = None;
        let mut visible: Vec<u64> = Vec::new();
        let (files, view, checked, focus) = (&self.files, &self.view, &self.checked, self.focus);
        let (thumbs, src) = (&mut self.thumbs, self.source.as_ref());
        ui.spacing_mut().item_spacing.y = gap;
        let out = sa.show_rows(ui, tile.y, rows, |ui, range| {
            for r in range.clone() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let k = r * cols + c;
                        let Some(&i) = view.get(k) else { break };
                        let f = &files[i as usize];
                        visible.push(f.id);
                        let thumb = src.and_then(|s| thumbs.get(s, f, ctx));
                        let (resp, cb) = tile_ui(
                            ui,
                            f,
                            tile,
                            checked.contains(&f.id),
                            focus == Some(f.id),
                            thumb,
                            p,
                            t,
                        );
                        if cb || resp.double_clicked() {
                            toggled = Some(f.id);
                        } else if resp.clicked() {
                            clicked = Some(i as usize);
                        }
                        resp.context_menu(|ui| {
                            if ui.button(t.recover_one).clicked() {
                                recover_one = Some(f.id);
                            }
                        });
                    }
                });
            }
            range
        });
        self.grid_rows = out.inner;
        self.thumbs.set_visible(visible);
        if let Some(id) = toggled {
            self.toggle_check(id);
        }
        if let Some(i) = clicked {
            self.set_focus(i, ctx);
        }
        if let Some(id) = recover_one {
            self.open_recover(vec![id]);
        }
    }

    // -------------------------------------------------------------- preview

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        let mut recover_one = None;
        egui::Panel::right("preview")
            .resizable(true)
            .default_size(280.0)
            .size_range(230.0..=560.0)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(egui::Margin::symmetric(16, 16)),
            )
            .show(ui, |ui| {
                let Some(f) = self
                    .focus
                    .and_then(|id| self.files.iter().find(|f| f.id == id))
                    .cloned()
                else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.3);
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 48.0, p.primary_soft);
                        theme::paint_file(ui.painter(), r.shrink(26.0), "", Category::Other, p);
                        ui.add_space(10.0);
                        ui.label(bold(t.preview).size(15.0).color(p.text));
                        ui.add(
                            egui::Label::new(RichText::new(t.preview_hint).color(p.muted)).wrap(),
                        );
                    });
                    return;
                };
                let ins = self
                    .inspection
                    .as_ref()
                    .filter(|(id, _)| *id == f.id)
                    .map(|(_, i)| i.clone());
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        self.preview_box(ui, &f, ins.as_ref());
                        ui.add_space(12.0);
                        ui.add(egui::Label::new(bold(&f.name).size(16.0).color(p.text)).wrap());
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            theme::cond_pill(ui, f.condition, t);
                            theme::pill(ui, origin_label(f.origin, t), p.primary, p.primary_soft);
                            theme::pill(
                                ui,
                                f.ext.to_uppercase(),
                                theme::type_color(&f.ext, f.category),
                                p.track,
                            );
                        });
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(t.cond_hint[f.condition as usize])
                                    .size(12.0)
                                    .color(p.muted),
                            )
                            .wrap(),
                        );
                        ui.add_space(10.0);
                        let enabled = self.recover.is_none();
                        let w = ui.available_width();
                        let b = egui::Button::new(semi(t.recover_one).color(egui::Color32::WHITE))
                            .fill(p.primary)
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(w, 36.0));
                        if ui.add_enabled(enabled, b).clicked() {
                            recover_one = Some(f.id);
                        }
                        ui.add_space(12.0);
                        ui.label(semi(t.details).size(13.0).color(p.muted));
                        ui.add_space(2.0);
                        self.details(ui, &f, ins.as_ref());
                        if let Some((_, PreviewState::Hex(s))) =
                            self.preview.as_ref().filter(|(id, _)| *id == f.id)
                        {
                            ui.add_space(8.0);
                            egui::CollapsingHeader::new(semi(t.hex).size(13.0)).show(ui, |ui| {
                                egui::ScrollArea::horizontal()
                                    .id_salt("hex")
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(s).monospace().size(10.5),
                                            )
                                            .extend(),
                                        );
                                    });
                            });
                        }
                    });
            });
        if let Some(id) = recover_one {
            self.open_recover(vec![id]);
        }
    }

    fn preview_box(&self, ui: &mut egui::Ui, f: &FoundFile, ins: Option<&Inspection>) {
        let t = self.t();
        let p = pal(ui);
        let w = ui.available_width();
        let state = self
            .preview
            .as_ref()
            .filter(|(id, _)| *id == f.id)
            .map(|x| &x.1);
        egui::Frame::new()
            .fill(p.faint)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(10)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(w - 18.0);
                let icon = |ui: &mut egui::Ui, note: Option<&str>| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(18.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(80.0, 100.0), Sense::hover());
                        theme::paint_file(ui.painter(), r, &f.ext, f.category, p);
                        if let Some(n) = note {
                            ui.add_space(8.0);
                            ui.label(RichText::new(n).size(12.0).color(p.muted));
                        }
                        ui.add_space(14.0);
                    });
                };
                match state {
                    Some(PreviewState::Loading) => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(60.0);
                            ui.add(egui::Spinner::new().size(26.0).color(p.primary));
                            ui.label(RichText::new(t.loading).size(12.0).color(p.muted));
                            ui.add_space(60.0);
                        });
                    }
                    Some(PreviewState::Image(tex, full)) => {
                        let s = tex.size_vec2();
                        let avail = ui.available_width();
                        let scale = (avail / s.x).min(320.0 / s.y).min(2.0);
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Image::new((tex.id(), s * scale)).corner_radius(6));
                            ui.label(
                                RichText::new(format!("{} × {} px", full.0, full.1))
                                    .size(11.5)
                                    .color(p.muted),
                            );
                        });
                    }
                    Some(PreviewState::Text(s)) => {
                        let is_doc = ins.is_some_and(|i| i.text.is_some());
                        if is_doc {
                            ui.label(semi(format!("📄 {}", t.doc_text)).size(12.0).color(p.muted));
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("doctext")
                            .max_height(300.0)
                            .show(ui, |ui| {
                                let text = if is_doc {
                                    RichText::new(s).size(13.0)
                                } else {
                                    RichText::new(s).monospace().size(11.5)
                                };
                                ui.add(egui::Label::new(text.color(p.text)).wrap());
                            });
                    }
                    Some(PreviewState::Hex(_)) => icon(ui, Some(t.no_preview)),
                    None => icon(ui, None),
                }
            });
    }

    fn details(&self, ui: &mut egui::Ui, f: &FoundFile, ins: Option<&Inspection>) {
        let t = self.t();
        let p = pal(ui);
        egui::Grid::new("details")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .striped(false)
            .show(ui, |ui| {
                let mut kv = |k: &str, v: String| {
                    ui.label(RichText::new(k).size(12.5).color(p.muted));
                    ui.add(egui::Label::new(RichText::new(v).size(12.5).color(p.text)).wrap());
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
                if let Some(i) = ins {
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
            ui.add_space(6.0);
            let named = !f.name.starts_with('f') || f.name.contains(' ');
            let note = if named {
                t.named_from_content
            } else {
                t.carved_name_note
            };
            ui.add(
                egui::Label::new(RichText::new(note).size(12.0).italics().color(p.muted)).wrap(),
            );
        }
    }

    // ----------------------------------------------------------- recover bar

    fn recover_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        let lang = self.lang;
        egui::Panel::bottom("recbar")
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = self.checked.len();
                    ui.label(
                        bold(format!("{} {}", fmt_num(n, lang), t.selected))
                            .size(14.0)
                            .color(p.text),
                    );
                    if n > 0 {
                        ui.label(
                            RichText::new(format!("·  {}", fmt_size(self.checked_bytes)))
                                .size(13.0)
                                .color(p.muted),
                        );
                    }
                    ui.add_space(10.0);
                    if ui
                        .add_enabled(!self.view.is_empty(), egui::Button::new(t.select_all))
                        .clicked()
                    {
                        for &i in &self.view {
                            self.checked.insert(self.files[i as usize].id);
                        }
                        self.recount_checked();
                    }
                    if ui
                        .add_enabled(n > 0, egui::Button::new(t.select_none))
                        .clicked()
                    {
                        self.checked.clear();
                        self.recount_checked();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if n > 0 {
                            format!("{}  ({})", t.recover, fmt_num(n, lang))
                        } else {
                            t.recover.to_string()
                        };
                        if theme::primary_button(ui, label, n > 0 && self.recover.is_none(), true)
                            .clicked()
                        {
                            let ids: Vec<u64> = self.checked.iter().copied().collect();
                            self.open_recover(ids);
                        }
                    });
                });
            });
    }
}

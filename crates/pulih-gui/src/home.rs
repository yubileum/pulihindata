//! Start page: where to recover from (drive cards, common folders, disk
//! images), optionally what to look for, and the sticky "Scan" bar.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use pulih_core::bytes::fmt_size;
use pulih_core::{list_devices, locate_folder, DeviceInfo, DeviceKind};

use crate::app::{App, Loc, Mode};
use crate::find;
use crate::i18n::T;
use crate::theme::{self, bold, pal, semi, DriveIcon};

const GAP: f32 = 14.0;
const CARD_H: f32 = 84.0;

struct Card {
    key: String,
    icon: DriveIcon,
    title: String,
    sub: String,
    hint: String,
    /// Used fraction of the volume.
    usage: Option<f32>,
}

/// What clicking a card does.
enum Pick {
    Device(DeviceInfo),
    Place(String, PathBuf),
    BrowseFolder,
    BrowseImage,
}

fn card_ui(ui: &mut egui::Ui, c: &Card, w: f32, selected: bool) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, CARD_H), Sense::click());
    let painter = ui.painter();
    let (fill, stroke) = if selected {
        (p.primary_soft, Stroke::new(1.5, p.primary))
    } else if resp.hovered() {
        (p.card, Stroke::new(1.0, p.primary.gamma_multiply(0.7)))
    } else {
        (p.card, Stroke::new(1.0, p.border))
    };
    painter.rect_filled(rect.translate(Vec2::new(0.0, 2.0)), 12, p.shadow);
    painter.rect(rect, 12, fill, stroke, StrokeKind::Inside);
    let icon = Rect::from_min_size(
        rect.min + Vec2::new(14.0, (CARD_H - 46.0) / 2.0),
        Vec2::splat(46.0),
    );
    theme::paint_drive(painter, icon, c.icon, p);
    let x = icon.right() + 12.0;
    let tw = rect.right() - 16.0 - x - if selected { 18.0 } else { 0.0 };
    let title = theme::elided(painter, &c.title, theme::font_bold(14.5), p.text, tw);
    let sub = theme::elided(
        painter,
        &c.sub,
        egui::FontId::proportional(12.0),
        p.muted,
        rect.right() - 16.0 - x,
    );
    let (ty, sy) = if c.usage.is_some() {
        (16.0, 37.0)
    } else {
        (24.0, 45.0)
    };
    painter.galley(egui::pos2(x, rect.top() + ty), title, p.text);
    painter.galley(egui::pos2(x, rect.top() + sy), sub, p.muted);
    if let Some(u) = c.usage {
        let bar = Rect::from_min_size(
            egui::pos2(x, rect.top() + 60.0),
            Vec2::new(rect.right() - 16.0 - x, 6.0),
        );
        painter.rect_filled(bar, 3, p.track);
        let col = if u > 0.92 {
            p.danger
        } else if u > 0.8 {
            p.warning
        } else {
            p.primary
        };
        painter.rect_filled(
            Rect::from_min_size(bar.min, Vec2::new(bar.width() * u.clamp(0.0, 1.0), 6.0)),
            3,
            col,
        );
    }
    if selected {
        theme::paint_check(
            painter,
            egui::pos2(rect.right() - 18.0, rect.top() + 18.0),
            9.0,
            p.primary,
        );
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if c.hint.is_empty() {
        resp
    } else {
        resp.on_hover_text(&c.hint)
    }
}

/// Lay cards out in equal columns; returns (index, double-clicked).
fn card_grid(ui: &mut egui::Ui, cards: &[Card], sel: Option<&str>) -> Option<(usize, bool)> {
    let avail = ui.available_width();
    let cols = ((avail + GAP) / (250.0 + GAP)).floor().max(1.0) as usize;
    let w = ((avail - GAP * (cols - 1) as f32) / cols as f32).floor();
    let mut out = None;
    for (r, row) in cards.chunks(cols).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (i, c) in row.iter().enumerate() {
                let resp = card_ui(ui, c, w, sel == Some(c.key.as_str()));
                if resp.double_clicked() {
                    out = Some((r * cols + i, true));
                } else if resp.clicked() {
                    out = Some((r * cols + i, false));
                }
            }
        });
        ui.add_space(GAP);
    }
    out
}

fn section_head(ui: &mut egui::Ui, text: &str, action: Option<&str>) -> bool {
    let p = pal(ui);
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(bold(text).size(15.0).color(p.text));
        if let Some(a) = action {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                clicked = ui
                    .add(egui::Button::new(semi(a).size(12.5).color(p.primary)).frame(false))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked();
            });
        }
    });
    ui.add_space(6.0);
    clicked
}

fn drive_title(d: &DeviceInfo, t: &T) -> String {
    let base = if !d.label.is_empty() {
        d.label.clone()
    } else if d.removable {
        t.removable_disk.to_string()
    } else {
        t.local_disk.to_string()
    };
    let letter = d.mount.trim_end_matches(['\\', '/']);
    if cfg!(windows) && letter.len() == 2 {
        format!("{base} ({letter})")
    } else {
        format!("{base} ({})", d.name)
    }
}

fn drive_card(d: &DeviceInfo, t: &T) -> Card {
    let size = fmt_size(d.size);
    match d.kind {
        DeviceKind::Partition => {
            let mut sub = match d.free {
                Some(free) if d.size > 0 => format!("{} {} {size}", fmt_size(free), t.free_of),
                _ => size,
            };
            if !d.fs.is_empty() {
                sub = format!("{} · {sub}", d.fs);
            }
            let mut hint = d.path.clone();
            if !d.mount.is_empty() && !cfg!(windows) {
                hint = format!("{hint}  →  {}", d.mount);
            }
            Card {
                key: d.path.clone(),
                icon: if d.removable {
                    DriveIcon::Removable
                } else {
                    DriveIcon::Partition
                },
                title: drive_title(d, t),
                sub,
                hint,
                usage: d
                    .free
                    .filter(|_| d.size > 0)
                    .map(|f| 1.0 - f as f32 / d.size as f32),
            }
        }
        DeviceKind::Disk => Card {
            key: d.path.clone(),
            icon: if d.removable {
                DriveIcon::Removable
            } else {
                DriveIcon::Disk
            },
            title: d.name.clone(),
            sub: format!("{size} · {}", t.whole_disk),
            hint: format!("{}\n{}", t.whole_disk_hint, d.path),
            usage: None,
        },
    }
}

impl App {
    pub fn home(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        self.scan_bar(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 32,
                                right: 32,
                                top: 28,
                                bottom: 24,
                            })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.home_body(ui);
                            });
                    });
            });
        let ready = self.sel.is_some() || !self.path_input.trim().is_empty();
        if ready && self.setup.is_none() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.start_scan();
        }
    }

    fn home_body(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        theme::title(ui, t.home_title, None);
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(t.home_sub).size(14.0).color(p.muted)).wrap());
        ui.add_space(12.0);
        theme::callout(ui, t.warn_write, p.warning, p.warning_soft);
        ui.add_space(14.0);
        self.find_card(ui);
        ui.add_space(22.0);

        let sel_key = self.sel.as_ref().map(|s| s.key.clone());
        let mut pick: Option<(Pick, bool)> = None;

        // Partitions / volumes first — the fast path, with names. Without
        // any (e.g. only raw disks), the physical disks lead instead.
        let has_parts = self.devices.iter().any(|d| d.kind == DeviceKind::Partition);
        let first = if has_parts || self.devices.is_empty() {
            t.sec_drives
        } else {
            t.sec_disks
        };
        if section_head(ui, first, Some(t.refresh)) {
            self.devices = list_devices();
            self.places = crate::app::known_places();
            return;
        }
        if self.devices.is_empty() {
            theme::callout(ui, t.no_devices, p.warning, p.warning_soft);
            ui.add_space(GAP);
        }
        let device_grid = |ui: &mut egui::Ui, kind: DeviceKind| -> Option<(Pick, bool)> {
            let list: Vec<&DeviceInfo> = self.devices.iter().filter(|d| d.kind == kind).collect();
            let cards: Vec<Card> = list.iter().map(|d| drive_card(d, t)).collect();
            let r = card_grid(ui, &cards, sel_key.as_deref())
                .map(|(i, dbl)| (Pick::Device(list[i].clone()), dbl));
            ui.add_space(8.0);
            r
        };
        let has_disks = self.devices.iter().any(|d| d.kind == DeviceKind::Disk);
        if has_parts {
            pick = pick.or(device_grid(ui, DeviceKind::Partition));
        } else if has_disks {
            pick = pick.or(device_grid(ui, DeviceKind::Disk));
        }

        if !self.places.is_empty() {
            section_head(ui, t.sec_places, None);
            let cards: Vec<Card> = self
                .places
                .iter()
                .map(|(i, path)| Card {
                    key: path.to_string_lossy().into_owned(),
                    icon: DriveIcon::Folder,
                    title: t.places[*i].to_string(),
                    sub: path.to_string_lossy().into_owned(),
                    hint: String::new(),
                    usage: None,
                })
                .collect();
            if let Some((i, dbl)) = card_grid(ui, &cards, sel_key.as_deref()) {
                let (n, path) = &self.places[i];
                pick = Some((Pick::Place(t.places[*n].to_string(), path.clone()), dbl));
            }
            ui.add_space(8.0);
        }

        if has_parts && has_disks {
            section_head(ui, t.sec_disks, None);
            pick = pick.or(device_grid(ui, DeviceKind::Disk));
        }

        section_head(ui, t.sec_other, None);
        // The browse cards show what was picked through them.
        let custom = self.sel.as_ref().filter(|s| {
            s.icon == DriveIcon::Image
                || (s.icon == DriveIcon::Folder
                    && !self
                        .places
                        .iter()
                        .any(|(_, p)| p.to_string_lossy() == s.key))
        });
        let folder_sel = custom.filter(|s| s.icon == DriveIcon::Folder);
        let image_sel = custom.filter(|s| s.icon == DriveIcon::Image);
        let cards = [
            Card {
                key: folder_sel.map_or_else(|| "?folder".into(), |s| s.key.clone()),
                icon: DriveIcon::Folder,
                title: folder_sel.map_or_else(|| t.pick_folder.to_string(), |s| s.title.clone()),
                sub: folder_sel.map_or_else(|| t.pick_folder_sub.to_string(), |s| s.sub.clone()),
                hint: String::new(),
                usage: None,
            },
            Card {
                key: image_sel.map_or_else(|| "?image".into(), |s| s.key.clone()),
                icon: DriveIcon::Image,
                title: image_sel.map_or_else(|| t.image_title.to_string(), |s| s.title.clone()),
                sub: image_sel.map_or_else(|| t.image_sub.to_string(), |s| s.sub.clone()),
                hint: t.drop_hint.to_string(),
                usage: None,
            },
        ];
        if let Some((i, dbl)) = card_grid(ui, &cards, sel_key.as_deref()) {
            if dbl && custom.is_some_and(|s| s.key == cards[i].key) {
                self.start_scan();
                return;
            }
            pick = Some((
                if i == 0 {
                    Pick::BrowseFolder
                } else {
                    Pick::BrowseImage
                },
                false,
            ));
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(t.manual_path).size(12.5).color(p.muted));
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text(t.image_hint)
                    .desired_width(ui.available_width().min(520.0)),
            );
            if r.changed() {
                self.sel = None;
                self.error = None;
            }
        });

        if let Some((pk, dbl)) = pick {
            if self.apply_pick(pk) && dbl {
                self.start_scan();
            }
        }
    }

    /// Turn a card click into a selected location.
    fn apply_pick(&mut self, pick: Pick) -> bool {
        let t = self.t();
        let folder_loc = |name: String, path: &Path| -> Result<Loc, String> {
            let shown = path.to_string_lossy().into_owned();
            let (dev, rel) =
                locate_folder(path).ok_or_else(|| format!("{} {shown}", t.open_failed))?;
            Ok(Loc {
                key: shown.clone(),
                path: dev.path.clone(),
                title: name,
                sub: format!("{shown}  ·  {}", drive_title(&dev, t)),
                icon: DriveIcon::Folder,
                folder: (!rel.is_empty()).then_some(rel),
            })
        };
        let loc = match pick {
            Pick::Device(d) => {
                let c = drive_card(&d, t);
                Ok(Loc {
                    key: d.path.clone(),
                    path: d.path.clone(),
                    title: c.title,
                    sub: c.sub,
                    icon: c.icon,
                    folder: None,
                })
            }
            Pick::Place(name, path) => folder_loc(name, &path),
            Pick::BrowseFolder => match rfd::FileDialog::new()
                .set_title(t.pick_folder)
                .pick_folder()
            {
                Some(path) => {
                    let name = path.file_name().map_or_else(
                        || path.to_string_lossy().into_owned(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    folder_loc(name, &path)
                }
                None => return false,
            },
            Pick::BrowseImage => {
                match rfd::FileDialog::new().set_title(t.image_title).pick_file() {
                    Some(path) => Ok(Loc::image(&path)),
                    None => return false,
                }
            }
        };
        match loc {
            Ok(l) => {
                self.sel = Some(l);
                self.path_input.clear();
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn find_card(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        theme::card(ui)
            .inner_margin(egui::Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 19.0, p.primary_soft);
                    paint_magnifier(ui.painter(), r.center(), 8.0, p.primary);
                    ui.vertical(|ui| {
                        ui.label(bold(t.find_title).size(15.5).color(p.text));
                        ui.label(RichText::new(t.find_sub).size(12.5).color(p.muted));
                    });
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let extra = self.find.exts.len()
                        + !self.find.folder.trim().is_empty() as usize
                        + (self.find.date != find::DatePreset::Any) as usize
                        + (self.find.min_mb > 0.0 || self.find.max_mb > 0.0) as usize;
                    let btn_w = 170.0;
                    ui.add(
                        egui::TextEdit::singleline(&mut self.find.names)
                            .hint_text(t.find_name_hint)
                            .desired_width(ui.available_width() - btn_w - 8.0)
                            .margin(egui::Margin::symmetric(12, 8))
                            .font(egui::FontId::proportional(14.5)),
                    )
                    .on_hover_text(t.find_name_help);
                    let label = format!(
                        "{}{}      ",
                        t.more_filters,
                        if extra > 0 {
                            format!("  ({extra})")
                        } else {
                            String::new()
                        }
                    );
                    let on = self.show_more || extra > 0;
                    let b =
                        egui::Button::new(semi(label).color(if on { p.primary } else { p.text }))
                            .fill(if on { p.primary_soft } else { p.card })
                            .stroke(Stroke::new(1.0, if on { p.primary } else { p.border }))
                            .min_size(Vec2::new(btn_w, 36.0));
                    let r = ui.add(b);
                    theme::paint_chevron(
                        ui.painter(),
                        r.rect.right_center() - Vec2::new(18.0, 0.0),
                        4.0,
                        if on { p.primary } else { p.text },
                        self.show_more,
                    );
                    if r.clicked() {
                        self.show_more = !self.show_more;
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(t.find_name_help).size(11.5).color(p.muted));
                if self.show_more {
                    ui.add_space(6.0);
                    ui.separator();
                    find::form(ui, &mut self.find, t);
                }
                if let Some(e) = self
                    .find
                    .target()
                    .wanted_exts()
                    .filter(|_| self.mode != Mode::Quick)
                {
                    ui.add_space(6.0);
                    ui.label(
                        semi(format!(
                            "🔎 {}: {}",
                            t.deep_limited,
                            e.join(", ").to_uppercase()
                        ))
                        .size(12.0)
                        .color(p.primary),
                    );
                }
                if self.find.is_active() {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{}: {}",
                                t.search_active,
                                find::summary(&self.find, t)
                            ))
                            .size(12.0)
                            .color(p.muted),
                        );
                        if ui
                            .add(
                                egui::Button::new(semi(t.reset_search).size(12.0).color(p.danger))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.find = find::Find::default();
                        }
                    });
                }
            });
    }

    /// Sticky bar at the bottom: chosen location, options, Scan.
    fn scan_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        egui::Panel::bottom("scanbar")
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(egui::Margin::symmetric(28, 14)),
            )
            .show(ui, |ui| {
                if let Some(e) = self.error.clone() {
                    theme::callout(ui, &format!("⚠  {e}"), p.danger, p.danger_soft);
                    ui.add_space(10.0);
                }
                ui.horizontal(|ui| {
                    let right_w = 430.0;
                    let left_w = (ui.available_width() - right_w).max(160.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(left_w, 48.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(42.0), Sense::hover());
                            let typed = self.path_input.trim();
                            let icon = self.sel.as_ref().map_or(DriveIcon::Image, |s| s.icon);
                            if self.sel.is_some() || !typed.is_empty() {
                                theme::paint_drive(ui.painter(), r, icon, p);
                            } else {
                                ui.painter().circle_stroke(
                                    r.center(),
                                    16.0,
                                    Stroke::new(1.5, p.border),
                                );
                            }
                            ui.vertical(|ui| {
                                ui.label(semi(t.location).size(11.5).color(p.muted));
                                let w = ui.available_width();
                                let (title, sub) = match &self.sel {
                                    Some(s) => (s.title.clone(), s.sub.clone()),
                                    None if !typed.is_empty() => (typed.to_string(), String::new()),
                                    None => (t.no_location.to_string(), String::new()),
                                };
                                let g = theme::elided(
                                    ui.painter(),
                                    &title,
                                    theme::font_bold(14.5),
                                    p.text,
                                    w,
                                );
                                ui.label(g);
                                if !sub.is_empty() {
                                    let g = theme::elided(
                                        ui.painter(),
                                        &sub,
                                        egui::FontId::proportional(11.5),
                                        p.muted,
                                        w,
                                    );
                                    ui.label(g);
                                }
                            });
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ready = self.sel.is_some() || !self.path_input.trim().is_empty();
                        if theme::primary_button(ui, format!("🔍  {}", t.scan_btn), ready, true)
                            .clicked()
                        {
                            self.start_scan();
                        }
                        ui.add_space(4.0);
                        let mode = t.modes[self.mode as usize];
                        theme::dropdown(ui, &format!("⚙  {}: {mode}", t.options), false, |ui| {
                            self.options_menu(ui)
                        });
                    });
                });
            });
    }

    fn options_menu(&mut self, ui: &mut egui::Ui) {
        let t = self.t();
        let p = pal(ui);
        ui.set_width(380.0);
        ui.label(
            bold(
                t.step_options
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' '),
            )
            .color(p.text),
        );
        ui.add_space(4.0);
        for (m, title, hint) in [
            (Mode::Both, t.mode_both, t.mode_both_hint),
            (Mode::Quick, t.mode_quick, t.mode_quick_hint),
            (Mode::Deep, t.mode_deep, t.mode_deep_hint),
        ] {
            ui.radio_value(&mut self.mode, m, semi(title));
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.add(egui::Label::new(RichText::new(hint).size(12.0).color(p.muted)).wrap());
            });
            ui.add_space(2.0);
        }
        ui.separator();
        ui.add_enabled_ui(self.mode != Mode::Quick, |ui| {
            ui.checkbox(&mut self.opts.free_only, t.free_only)
                .on_hover_text(t.free_only_hint);
            ui.checkbox(&mut self.opts.byte_level, t.byte_level)
                .on_hover_text(t.byte_level_hint);
            ui.add_space(4.0);
            ui.label(semi(t.advanced).size(12.5).color(p.muted));
            ui.add(egui::Slider::new(&mut self.opts.threads, 1..=16).text(t.threads));
            ui.add(egui::Slider::new(&mut self.opts.chunk_mb, 4..=64).text(t.chunk));
        });
    }
}

/// Magnifying glass outline.
pub fn paint_magnifier(painter: &egui::Painter, c: egui::Pos2, r: f32, color: egui::Color32) {
    let s = Stroke::new((r / 4.0).max(1.6), color);
    let lens = c - Vec2::splat(r * 0.2);
    painter.circle_stroke(lens, r * 0.75, s);
    let d = std::f32::consts::FRAC_1_SQRT_2 * r * 0.75;
    painter.line_segment(
        [lens + Vec2::splat(d), lens + Vec2::splat(d + r * 0.6)],
        Stroke::new(s.width * 1.3, color),
    );
}

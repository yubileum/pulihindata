//! Modal dialogs: where to save recovered files, progress, and the result.

use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

use eframe::egui::{self, Align, Id, Layout, RichText, Sense, Stroke, Vec2};
use pulih_core::bytes::fmt_size;

use crate::app::{fmt_num, open_folder, App};
use crate::theme::{self, bold, pal, semi};

/// Round badge with a down-into-tray "save" glyph.
fn paint_save_badge(ui: &mut egui::Ui) {
    let p = pal(ui);
    let (r, _) = ui.allocate_exact_size(Vec2::splat(46.0), Sense::hover());
    let c = r.center();
    let painter = ui.painter();
    painter.circle_filled(c, 23.0, p.primary_soft);
    let s = Stroke::new(2.4, p.primary);
    painter.line_segment([c + Vec2::new(0.0, -10.0), c + Vec2::new(0.0, 4.0)], s);
    painter.line_segment([c + Vec2::new(-6.0, -2.0), c + Vec2::new(0.0, 4.5)], s);
    painter.line_segment([c + Vec2::new(6.0, -2.0), c + Vec2::new(0.0, 4.5)], s);
    painter.line_segment([c + Vec2::new(-10.0, 4.0), c + Vec2::new(-10.0, 10.0)], s);
    painter.line_segment([c + Vec2::new(-10.0, 10.0), c + Vec2::new(10.0, 10.0)], s);
    painter.line_segment([c + Vec2::new(10.0, 10.0), c + Vec2::new(10.0, 4.0)], s);
}

impl App {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if self.recover.is_some() {
            self.recover_dialog(ctx);
        } else if self.setup.is_some() {
            self.setup_dialog(ctx);
        }
    }

    fn setup_dialog(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let lang = self.lang;
        let (mut cancel, mut go, mut browse) = (false, false, false);
        let resp = egui::Modal::new(Id::unique("recover_setup")).show(ctx, |ui| {
            let p = pal(ui);
            ui.set_width(500.0);
            let Some(s) = &mut self.setup else { return };
            ui.horizontal(|ui| {
                paint_save_badge(ui);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(bold(t.dlg_title).size(19.0).color(p.text));
                    ui.label(
                        RichText::new(format!(
                            "{} file  ·  {}",
                            fmt_num(s.ids.len(), lang),
                            fmt_size(s.bytes)
                        ))
                        .size(13.0)
                        .color(p.muted),
                    );
                });
            });
            ui.add_space(12.0);
            theme::callout(ui, t.dlg_sub, p.primary, p.primary_soft);
            ui.add_space(14.0);
            ui.label(semi(t.dest).size(13.0).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let empty = self.out_dir.trim().is_empty();
                let b = if empty {
                    theme::primary_button(ui, t.browse, true, false)
                } else {
                    theme::soft_button(ui, t.browse)
                };
                browse = b.clicked();
                ui.add(
                    egui::TextEdit::singleline(&mut self.out_dir)
                        .hint_text(t.save_to_hint)
                        .desired_width(ui.available_width())
                        .margin(egui::Margin::symmetric(10, 7)),
                );
            });
            ui.add_space(6.0);
            ui.checkbox(&mut self.keep_folders, t.keep_folders);
            if s.same_disk {
                ui.add_space(10.0);
                theme::callout(ui, &format!("⚠  {}", t.same_disk), p.danger, p.danger_soft);
                ui.add_space(4.0);
                ui.checkbox(
                    &mut s.accept_risk,
                    RichText::new(t.continue_anyway).color(p.danger),
                );
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ok = !self.out_dir.trim().is_empty() && (!s.same_disk || s.accept_risk);
                go = theme::primary_button(ui, t.recover, ok, false).clicked();
                cancel = ui
                    .add(egui::Button::new(t.cancel).min_size(Vec2::new(0.0, 32.0)))
                    .clicked();
            });
        });
        if browse {
            self.pick_out_dir();
        }
        self.check_same_disk();
        if cancel || (resp.should_close() && !browse) {
            self.setup = None;
        } else if go {
            if let Some(s) = self.setup.take() {
                self.run_recover(PathBuf::from(self.out_dir.trim()), s.ids);
            }
        }
    }

    fn recover_dialog(&mut self, ctx: &egui::Context) {
        let t = self.t();
        let lang = self.lang;
        let mut close = false;
        let Some(j) = &self.recover else { return };
        egui::Modal::new(Id::unique("recover_job")).show(ctx, |ui| {
            let p = pal(ui);
            ui.set_width(470.0);
            match &j.report {
                None => {
                    let pr = &j.progress;
                    let (fd, ft) = (pr.files_done.load(Relaxed), pr.files_total.load(Relaxed));
                    let (bd, bt) = (pr.bytes_done.load(Relaxed), pr.bytes_total.load(Relaxed));
                    ui.horizontal(|ui| {
                        paint_save_badge(ui);
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.label(bold(t.recovering).size(18.0).color(p.text));
                            ui.label(
                                RichText::new(j.out.display().to_string())
                                    .size(12.0)
                                    .color(p.muted),
                            );
                        });
                    });
                    ui.add_space(14.0);
                    let frac = if bt > 0 { bd as f32 / bt as f32 } else { 0.0 };
                    theme::progress(ui, frac, 10.0, true);
                    ui.add_space(6.0);
                    let el = j.started.elapsed().as_secs_f64().max(0.001);
                    ui.horizontal(|ui| {
                        ui.label(
                            bold(format!("{:.0}%", frac * 100.0))
                                .size(13.0)
                                .color(p.primary),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} / {} file  ·  {} / {}  ·  {}/s",
                                fmt_num(fd, lang),
                                fmt_num(ft, lang),
                                fmt_size(bd),
                                fmt_size(bt),
                                fmt_size((bd as f64 / el) as u64)
                            ))
                            .size(12.5)
                            .color(p.muted),
                        );
                    });
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(semi(t.cancel).color(p.danger))
                            .fill(p.danger_soft)
                            .stroke(Stroke::NONE);
                        if ui.add(b.min_size(Vec2::new(0.0, 32.0))).clicked() {
                            pr.cancel.store(true, Relaxed);
                        }
                    });
                }
                Some(r) => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(6.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 32.0, p.success_soft);
                        theme::paint_check(ui.painter(), rect.center(), 20.0, p.success);
                        ui.add_space(10.0);
                        ui.label(bold(t.rec_done).size(20.0).color(p.text));
                        ui.label(
                            semi(format!(
                                "{} {}  ·  {}",
                                fmt_num(r.written, lang),
                                t.recovered,
                                fmt_size(r.bytes)
                            ))
                            .size(14.0)
                            .color(p.text),
                        );
                        ui.add_space(2.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(j.out.display().to_string())
                                    .size(12.0)
                                    .color(p.muted),
                            )
                            .wrap(),
                        );
                    });
                    if !r.failed.is_empty() {
                        ui.add_space(10.0);
                        theme::callout(
                            ui,
                            &format!("{} {}", fmt_num(r.failed.len(), lang), t.failed),
                            p.danger,
                            p.danger_soft,
                        );
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for (n, e) in r.failed.iter().take(200) {
                                    ui.label(
                                        RichText::new(format!("{n}: {e}"))
                                            .size(12.0)
                                            .color(p.muted),
                                    );
                                }
                            });
                    }
                    if r.bad_sectors > 0 {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("{}: {}", t.bad_sectors, r.bad_sectors))
                                .color(p.warning),
                        );
                    }
                    ui.add_space(16.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::primary_button(ui, t.view_recovered, true, false).clicked() {
                            open_folder(&j.out);
                        }
                        if ui
                            .add(egui::Button::new(t.finish).min_size(Vec2::new(0.0, 32.0)))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                }
            }
        });
        if close {
            self.recover = None;
        }
    }
}

//! The Check sketch window: what the sketch checker finds, to show and fix one at a time or
//! all at once, with markers on the canvas while it is open.

use crate::*;
use drift_edit::checker::Issue;

/// The most issues listed (the rest are counted, and fixed by Fix all).
const LISTED: usize = 200;

/// The open Check sketch window.
#[derive(Default)]
pub(crate) struct SketchCheck {
    /// What was found, as of a document revision (a new tolerance bumps it).
    found: Option<(u64, Vec<Issue>)>,
}

impl DriftApp {
    /// Open the Check sketch window (from the Edit menu, the palette or an import).
    pub(crate) fn open_sketch_check(&mut self) {
        self.drawing.sketch_check = Some(SketchCheck::default());
    }

    /// The issues for the document as it is now (found again only when it changes).
    pub(crate) fn sketch_issues(&mut self) -> Vec<Issue> {
        let revision = self.drawing.editor.doc.revision();
        let tolerance = self.drawing.editor.doc.rules().gap_tolerance;
        let Some(check) = &mut self.drawing.sketch_check else {
            return Vec::new();
        };
        match &check.found {
            Some((r, issues)) if *r == revision => issues.clone(),
            _ => {
                let issues = self.drawing.editor.check_sketch(tolerance);
                check.found = Some((revision, issues.clone()));
                issues
            }
        }
    }

    /// The drawing's gap tolerance, as Check sketch and Drawing settings both edit it: a drag
    /// or a typed value is one undo step.
    pub(crate) fn gap_tolerance_row(&mut self, ui: &mut egui::Ui) {
        let units = self.drawing.editor.doc.units();
        let per = units.length.mm_per_unit();
        let mut shown = self.drawing.editor.doc.rules().gap_tolerance / per;
        let mut response = None;
        theme::row(ui, "Gaps up to", |ui| {
            response = Some(
                ui.add(
                    egui::DragValue::new(&mut shown)
                        .speed(0.01)
                        .range(0.0001..=1000.0)
                        .max_decimals(4)
                        .suffix(format!(" {}", units.length.symbol())),
                )
                .on_hover_text(
                    "Check sketch takes ends closer than this to be meant to meet, and leaves ends \
                 further apart as they are",
                ),
            );
        });
        if let Some(response) = response.filter(egui::Response::changed) {
            // Still the same drag, or the same typing: one step.
            let continuing = (response.dragged() && !response.drag_started())
                || (response.has_focus() && !response.gained_focus());
            self.drawing
                .editor
                .change_rules_merging(continuing, |r| r.gap_tolerance = shown * per);
        }
    }

    pub(crate) fn sketch_check_window(&mut self, ctx: &egui::Context) {
        if self.drawing.sketch_check.is_none() {
            return;
        }
        let issues = self.sketch_issues();
        let units = self.drawing.editor.doc.units();
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let mut close = false;
        let mut show: Option<Issue> = None;
        let mut fix: Option<Issue> = None;
        let mut fix_all = false;
        theme::dialog(ctx, "Check sketch")
            .id(egui::Id::new("sketch_check"))
            .open(&mut open)
            .pivot(egui::Align2::LEFT_TOP)
            // Over the canvas's top right, clear of the side panels.
            .default_pos(self.drawing.view.canvas_rect.right_top() + egui::vec2(-446.0, 14.0))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                if theme::dialog_title(ui, "Check sketch", true) {
                    close = true;
                }
                self.gap_tolerance_row(ui);
                ui.add_space(4.0);
                if issues.is_empty() {
                    ui.label("No gaps, ends short of a curve, or curves of no length.");
                } else {
                    let gaps = issues
                        .iter()
                        .filter(|i| matches!(i, Issue::Gap { .. }))
                        .count();
                    let plural = |n: usize, one: &str, many: &str| {
                        format!("{n} {}", if n == 1 { one } else { many })
                    };
                    let other = issues.len() - gaps;
                    let mut found = Vec::new();
                    if gaps > 0 {
                        found.push(plural(gaps, "gap between ends", "gaps between ends"));
                    }
                    if other > 0 {
                        found.push(plural(other, "other issue", "other issues"));
                    }
                    ui.label(format!("Found {}.", found.join(" and ")));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (k, issue) in issues.iter().take(LISTED).enumerate() {
                                ui.push_id(k, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(issue.label());
                                        if let Some(d) = issue.distance() {
                                            ui.label(
                                                egui::RichText::new(units.format_readout(d)).weak(),
                                            );
                                        }
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.small_button("Fix").clicked() {
                                                    fix = Some(*issue);
                                                }
                                                if ui
                                                    .small_button("Show")
                                                    .on_hover_text("Pick it and zoom to it")
                                                    .clicked()
                                                {
                                                    show = Some(*issue);
                                                }
                                            },
                                        );
                                    });
                                });
                            }
                            if issues.len() > LISTED {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "and {} more",
                                        issues.len() - LISTED
                                    ))
                                    .weak(),
                                );
                            }
                        });
                }
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    if ui
                        .add_enabled(
                            !issues.is_empty(),
                            egui::Button::new(format!("Fix all ({})", issues.len())),
                        )
                        .on_hover_text(
                            "Join each gap, pull each short end onto its curve and remove \
                             curves of no length, as one undo step. Any that would break a \
                             relation or dimension are left",
                        )
                        .clicked()
                    {
                        fix_all = true;
                    }
                });
            });
        if let Some(issue) = show {
            self.show_issue(&issue);
        }
        if let Some(issue) = fix {
            if self.drawing.editor.fix_issue(&issue) {
                self.set_status(
                    format!("Fixed: {}", issue.label().to_lowercase()),
                    false,
                    now,
                );
            } else if let Some(notice) = self.drawing.editor.out.notice.take() {
                self.set_status(notice, true, now);
            }
        }
        if fix_all {
            let (fixed, left) = self.drawing.editor.fix_issues(&issues);
            let count = |n: usize| format!("{n} issue{}", if n == 1 { "" } else { "s" });
            let text = if left == 0 {
                format!("Fixed {}", count(fixed))
            } else {
                format!(
                    "Fixed {}; {left} left (they conflict with the relations or dimensions \
                     there)",
                    count(fixed)
                )
            };
            self.set_status(text, left > 0 && fixed == 0, now);
        }
        if close || !open {
            self.drawing.sketch_check = None;
        }
    }

    /// Pick what an issue concerns and zoom to it.
    fn show_issue(&mut self, issue: &Issue) {
        self.abort_operation();
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        self.drawing.editor.selection = issue
            .entities()
            .into_iter()
            .filter(|id| self.drawing.editor.doc.contains(*id))
            .collect();
        if self.zoom_to_selection()
            && let Some(bounds) = self.drawing.view.fit_target
        {
            // Close enough to see the gap, not just the curves around it.
            let pad = DVec2::splat(issue.distance().unwrap_or(1.0).max(1.0) * 20.0);
            let at = issue_at(&self.drawing.editor.doc, issue).unwrap_or(bounds.center());
            self.drawing.view.fit_target = Some(Bounds {
                min: at - pad,
                max: at + pad,
            });
        }
    }

    /// Rings round what the open Check sketch window lists.
    pub(crate) fn paint_check_markers(&mut self, painter: &egui::Painter, map: &ScreenMap) {
        if self.drawing.sketch_check.is_none() {
            return;
        }
        let amber = egui::Color32::from_rgb(0xff, 0x9f, 0x0a);
        for issue in self.sketch_issues().iter().take(LISTED * 5) {
            if let Some(at) = issue_at(&self.drawing.editor.doc, issue) {
                let at = map.to_screen(at);
                painter.circle_stroke(at, 9.0, egui::Stroke::new(2.0, amber));
            }
        }
    }
}

/// Where an issue is: the middle of a gap, a short end, a curve of no length.
fn issue_at(doc: &drift_core::Document, issue: &Issue) -> Option<DVec2> {
    match *issue {
        Issue::Gap { keep, join, .. } => {
            Some((doc.point(keep).ok()? + doc.point(join).ok()?) / 2.0)
        }
        Issue::Short { point, .. } => doc.point(point).ok(),
        Issue::ZeroLength { curve } => doc.shape(curve).map(|s| s.bounds().center()),
    }
}

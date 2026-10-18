//! The Check sketch window: what the sketch checker finds, to show and fix one at a time or
//! all at once, with markers on the canvas while it is open.

use crate::checker::Issue;
use crate::*;

/// The most issues listed (the rest are counted, and fixed by Fix all).
const LISTED: usize = 200;

/// The open Check sketch window.
#[derive(Default)]
pub(crate) struct SketchCheck {
    /// What was found, as of a document revision and tolerance.
    found: Option<(u64, f64, Vec<Issue>)>,
}

impl DriftApp {
    /// Open the Check sketch window (from the Edit menu, the palette or an import).
    pub(crate) fn open_sketch_check(&mut self) {
        self.sketch_check = Some(SketchCheck::default());
    }

    /// The issues for the document as it is now (found again only when it changes).
    pub(crate) fn sketch_issues(&mut self) -> Vec<Issue> {
        let revision = self.editor.doc.revision();
        let tolerance = self.gap_tolerance;
        let Some(check) = &mut self.sketch_check else {
            return Vec::new();
        };
        match &check.found {
            Some((r, t, issues)) if *r == revision && *t == tolerance => issues.clone(),
            _ => {
                let issues = self.editor.check_sketch(tolerance);
                check.found = Some((revision, tolerance, issues.clone()));
                issues
            }
        }
    }

    pub(crate) fn sketch_check_window(&mut self, ctx: &egui::Context) {
        if self.sketch_check.is_none() {
            return;
        }
        let issues = self.sketch_issues();
        let units = self.editor.doc.units();
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let mut close = false;
        let mut show: Option<Issue> = None;
        let mut fix: Option<Issue> = None;
        let mut fix_all = false;
        let mut tolerance = self.gap_tolerance;
        theme::dialog(ctx, "Check sketch")
            .id(egui::Id::new("sketch_check"))
            .open(&mut open)
            .pivot(egui::Align2::LEFT_TOP)
            // Over the canvas's top right, clear of the side panels.
            .default_pos(self.canvas_rect.right_top() + egui::vec2(-446.0, 14.0))
            .show(ctx, |ui| {
                ui.set_width(400.0);
                if theme::dialog_title(ui, "Check sketch", true) {
                    close = true;
                }
                theme::row(ui, "Gaps up to", |ui| {
                    let per = units.length.mm_per_unit();
                    let mut shown = tolerance / per;
                    if ui
                        .add(
                            egui::DragValue::new(&mut shown)
                                .speed(0.01)
                                .range(0.0001..=1000.0)
                                .max_decimals(4)
                                .suffix(format!(" {}", units.length.symbol())),
                        )
                        .on_hover_text(
                            "Ends closer than this are taken to be meant to meet. Ends further \
                             apart are left as they are",
                        )
                        .changed()
                    {
                        tolerance = shown * per;
                    }
                });
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
        self.gap_tolerance = tolerance;
        if let Some(issue) = show {
            self.show_issue(&issue);
        }
        if let Some(issue) = fix {
            if self.editor.fix_issue(&issue) {
                self.set_status(
                    format!("Fixed: {}", issue.label().to_lowercase()),
                    false,
                    now,
                );
            } else if let Some(notice) = self.editor.notice.take() {
                self.set_status(notice, true, now);
            }
        }
        if fix_all {
            let (fixed, left) = self.editor.fix_issues(&issues);
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
            self.sketch_check = None;
        }
    }

    /// Pick what an issue concerns and zoom to it.
    fn show_issue(&mut self, issue: &Issue) {
        self.abort_operation();
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        self.editor.selection = issue
            .entities()
            .into_iter()
            .filter(|id| self.editor.doc.contains(*id))
            .collect();
        if self.zoom_to_selection()
            && let Some(bounds) = self.fit_target
        {
            // Close enough to see the gap, not just the curves around it.
            let pad = DVec2::splat(issue.distance().unwrap_or(1.0).max(1.0) * 20.0);
            let at = issue_at(&self.editor.doc, issue).unwrap_or(bounds.center());
            self.fit_target = Some(Bounds {
                min: at - pad,
                max: at + pad,
            });
        }
    }

    /// Rings round what the open Check sketch window lists.
    pub(crate) fn paint_check_markers(&mut self, painter: &egui::Painter, map: &ScreenMap) {
        if self.sketch_check.is_none() {
            return;
        }
        let amber = egui::Color32::from_rgb(0xff, 0x9f, 0x0a);
        for issue in self.sketch_issues().iter().take(LISTED * 5) {
            if let Some(at) = issue_at(&self.editor.doc, issue) {
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

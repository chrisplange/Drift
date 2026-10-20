//! The browser demo (the `demo` feature): a banner saying nothing is saved, and a note in
//! place of every save, export and print. The code that writes files isn't in this build at
//! all, so these are all there is behind those menu items.

use crate::*;

/// The website Drift is downloaded from, as the build was given it (`DRIFT_SITE_URL`).
const SITE_URL: &str = match option_env!("DRIFT_SITE_URL") {
    Some(url) if !url.is_empty() => url,
    _ => "https://drift-website-c9y.pages.dev",
};

/// The download page.
pub(crate) fn download_url() -> String {
    format!("{}/download/", SITE_URL.trim_end_matches('/'))
}

#[derive(Default)]
pub(crate) struct Demo {
    /// What was asked for that the demo can't do ("Saving", "Exporting a PDF"), while its note
    /// is up.
    note: Option<&'static str>,
    banner_closed: bool,
}

/// Open the download page in a new tab, leaving the demo (and the drawing) as it is.
fn open_download(ctx: &egui::Context) {
    ctx.open_url(egui::OpenUrl::new_tab(download_url()));
}

impl DriftApp {
    /// In place of a save, export or print: say it isn't in the demo, and where Drift is.
    pub(crate) fn not_in_demo(&mut self, what: &'static str) {
        self.demo.note = Some(what);
    }

    /// The slim banner over the menu bar, until it is closed.
    pub(crate) fn demo_banner(&mut self, ui: &mut egui::Ui) {
        if self.demo.banner_closed {
            return;
        }
        let dark = ui.visuals().dark_mode;
        let frame = egui::Frame::new()
            .fill(theme::accent(dark))
            .inner_margin(egui::Margin::symmetric(12, 3));
        egui::Panel::top("demo_banner").frame(frame).show(ui, |ui| {
            ui.horizontal(|ui| {
                let white = egui::Color32::WHITE;
                ui.label(egui::RichText::new("Demo: nothing is saved.").color(white));
                let link = ui.add(
                    egui::Label::new(
                        egui::RichText::new("Download Drift")
                            .color(white)
                            .strong()
                            .underline(),
                    )
                    .sense(egui::Sense::click()),
                );
                if link
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(download_url())
                    .clicked()
                {
                    open_download(ui.ctx());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui.add(
                        egui::Button::new(egui::RichText::new("×").color(white))
                            .frame(false)
                            .small(),
                    );
                    if close.on_hover_text("Hide this banner").clicked() {
                        self.demo.banner_closed = true;
                    }
                });
            });
        });
    }

    /// The note shown in place of a save, export or print.
    pub(crate) fn demo_note(&mut self, ctx: &egui::Context) {
        let Some(what) = self.demo.note else {
            return;
        };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("not_in_demo"))
            .frame(theme::dialog_frame(&ctx.style_of(ctx.theme())))
            .show(ctx, |ui| {
                ui.set_width(360.0);
                if theme::dialog_title(ui, "Not in the demo", true) {
                    close = true;
                }
                ui.label(format!(
                    "{what} isn't in the browser demo: nothing leaves it. Download Drift to \
                     save, export and print your drawings."
                ));
                theme::dialog_buttons(ui, |ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    let dark = ui.visuals().dark_mode;
                    if ui
                        .add(theme::primary_button("Download Drift", dark))
                        .on_hover_text(download_url())
                        .clicked()
                    {
                        open_download(ui.ctx());
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            self.demo.note = None;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_download_link_is_the_sites_download_page() {
        let url = super::download_url();
        assert!(url.starts_with("https://"), "{url}");
        assert!(
            url.ends_with("/download/") && !url.ends_with("//download/"),
            "{url}"
        );
    }
}

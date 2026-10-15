//! Updates (native only): the check at start and from Help → Check for updates…, the notice
//! that offers a newer release, and Preferences → Updates.
//!
//! The network runs on a worker thread; it reports over a channel that each frame reads, so
//! nothing here waits on it. A failure is only a status message.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use semver::Version;

use crate::update::install::{self, Install, Plan};
use crate::update::{self, Platform, Release};
use crate::*;

/// What the worker thread reports.
pub(crate) enum Msg {
    Checked(Result<Option<Release>, String>),
    Progress(u64, Option<u64>),
    Prepared(Result<Plan, String>),
}

#[derive(Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Checking,
    /// `release` is newer than this.
    Available,
    Downloading {
        got: u64,
        total: Option<u64>,
    },
    /// Downloaded, verified and ready to run once Drift closes.
    Ready(Plan),
}

pub(crate) struct Updates {
    /// Check once when Drift starts (a preference).
    pub(crate) auto_check: bool,
    /// The release the user chose to skip: it isn't offered at start, newer ones are.
    pub(crate) skipped: Option<Version>,
    /// The check at start has been made (or isn't wanted).
    launched: bool,
    /// The check in progress was asked for (so "no update" and failures are told).
    manual: bool,
    pub(crate) phase: Phase,
    /// The newer release found.
    pub(crate) release: Option<Release>,
    /// The notice is showing.
    pub(crate) show: bool,
    /// "Update now" was clicked: install as soon as the download is ready.
    install_when_ready: bool,
    rx: Option<mpsc::Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
    /// How this copy was installed: whether it can update itself.
    pub(crate) install: Install,
    /// The update to start as Drift exits.
    pub(crate) on_exit: Option<Plan>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            auto_check: true,
            skipped: None,
            // Tests never reach the network.
            launched: cfg!(test),
            manual: false,
            phase: Phase::Idle,
            release: None,
            show: false,
            install_when_ready: false,
            rx: None,
            cancel: Default::default(),
            install: Install::current(),
            on_exit: None,
        }
    }
}

impl Updates {
    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Downloading { .. })
    }

    /// The release can be downloaded and installed here (rather than by hand).
    pub(crate) fn installable(&self) -> bool {
        self.install.can_update()
            && self.release.as_ref().is_some_and(|r| {
                Platform::current().is_some_and(|p| update::pick_asset(r, p).is_some())
            })
    }

    /// Run `work` on a worker thread, which reports through `send` and wakes the UI.
    fn spawn(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(&dyn Fn(Msg)) + Send + 'static,
    ) -> bool {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("drift-update".into())
            .spawn(move || {
                let send = |msg: Msg| {
                    let _ = tx.send(msg);
                    ctx.request_repaint();
                };
                work(&send);
            })
            .is_ok();
        if started {
            self.rx = Some(rx);
        }
        started
    }
}

/// Megabytes, for the download progress.
fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

impl DriftApp {
    /// Each frame: start the check at launch, and take the worker's news.
    pub(crate) fn updates_tick(&mut self, ctx: &egui::Context, now: f64) {
        if !self.updates.launched {
            self.updates.launched = true;
            if self.updates.auto_check && self.soak.is_none() {
                self.check_for_updates(ctx, false);
            }
        }
        let messages: Vec<Msg> = match &self.updates.rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for msg in messages {
            self.update_message(msg, now);
        }
    }

    pub(crate) fn update_message(&mut self, msg: Msg, now: f64) {
        let manual = self.updates.manual;
        let u = &mut self.updates;
        match msg {
            Msg::Checked(Ok(Some(release))) => {
                u.phase = Phase::Idle;
                if !manual && u.skipped.as_ref() == Some(&release.version) {
                    return;
                }
                u.release = Some(release);
                u.phase = Phase::Available;
                u.show = true;
            }
            Msg::Checked(Ok(None)) => {
                u.phase = Phase::Idle;
                if manual {
                    let text = format!("Drift {} is the newest version", env!("CARGO_PKG_VERSION"));
                    self.set_status(text, false, now);
                }
            }
            Msg::Checked(Err(e)) => {
                u.phase = Phase::Idle;
                log::info!("update check: {e}");
                self.set_status(format!("Couldn't check for updates: {e}"), manual, now);
            }
            Msg::Progress(got, total) => {
                if let Phase::Downloading { .. } = u.phase {
                    u.phase = Phase::Downloading { got, total };
                }
            }
            Msg::Prepared(Ok(plan)) => {
                u.phase = Phase::Ready(plan);
                u.show = true;
                if std::mem::take(&mut u.install_when_ready) {
                    self.request(Pending::Update);
                }
            }
            Msg::Prepared(Err(e)) => {
                u.phase = Phase::Available;
                u.install_when_ready = false;
                if !u.cancel.load(Ordering::Relaxed) {
                    self.set_status(format!("Couldn't update Drift: {e}"), true, now);
                }
            }
        }
    }

    /// Ask GitHub for a newer release (`manual`: the user asked, so say what came of it).
    pub(crate) fn check_for_updates(&mut self, ctx: &egui::Context, manual: bool) {
        let u = &mut self.updates;
        if u.busy() {
            return;
        }
        if manual && matches!(u.phase, Phase::Ready(_) | Phase::Available) {
            // Already found: show it again.
            u.show = true;
            return;
        }
        u.manual = manual;
        let current = update::current_version();
        if u.spawn(ctx, move |send| {
            let agent = update::net::Agent::new();
            send(Msg::Checked(update::check(&agent, &current)));
        }) {
            u.phase = Phase::Checking;
        }
    }

    /// Download and verify the release, and get it ready to install.
    fn start_download(&mut self, ctx: &egui::Context) {
        let u = &mut self.updates;
        let (Some(release), Some(platform)) = (u.release.clone(), Platform::current()) else {
            return;
        };
        if u.busy() {
            return;
        }
        let install = u.install.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        u.cancel = cancel.clone();
        if u.spawn(ctx, move |send| {
            let agent = update::net::Agent::new();
            let dir = update::download_dir(&release.version);
            let mut last = 0;
            let result = update::download(
                &agent,
                &release,
                platform,
                update::RELEASE_KEY,
                &dir,
                &mut |got, total| {
                    // Every quarter megabyte is plenty for a progress bar.
                    if got - last >= 256 << 10 || Some(got) == total {
                        last = got;
                        send(Msg::Progress(got, total));
                    }
                    !cancel.load(Ordering::Relaxed)
                },
            );
            send(Msg::Prepared(
                result.and_then(|path| install::prepare(&path, &install)),
            ));
        }) {
            u.phase = Phase::Downloading {
                got: 0,
                total: None,
            };
        }
    }

    /// Close Drift and install the downloaded update (once unsaved work has been dealt with).
    pub(crate) fn install_update_and_quit(&mut self) {
        if let Phase::Ready(plan) = &self.updates.phase {
            self.updates.on_exit = Some(plan.clone());
            self.quit_confirmed = true;
        }
    }

    /// Help menu items.
    pub(crate) fn updates_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let u = &self.updates;
        if let (Phase::Ready(_), Some(r)) = (&u.phase, &u.release) {
            if ui
                .button(format!("Restart to update to {}", r.version))
                .clicked()
            {
                self.request(Pending::Update);
            }
            return;
        }
        let label = match u.phase {
            Phase::Checking => "Checking for updates…",
            Phase::Downloading { .. } => "Downloading the update…",
            _ => "Check for updates…",
        };
        if ui
            .add_enabled(!u.busy(), egui::Button::new(label))
            .clicked()
        {
            self.check_for_updates(&ctx, true);
        }
    }

    /// The notice offering a newer release, at the bottom right; it never blocks the window.
    pub(crate) fn update_notice(&mut self, ctx: &egui::Context) {
        let u = &self.updates;
        let Some(release) = u.release.clone() else {
            return;
        };
        if !u.show || matches!(u.phase, Phase::Idle | Phase::Checking) {
            return;
        }
        let installable = u.installable();
        let mut action = None;
        egui::Window::new("Update available")
            .id(egui::Id::new("update_notice"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -40.0])
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .show(ctx, |ui| {
                ui.set_width(340.0);
                match &self.updates.phase {
                    Phase::Ready(_) => {
                        ui.heading(format!("Drift {} is ready to install", release.version));
                        ui.add_space(4.0);
                        ui.label(
                            "Downloaded and checked: it is signed with Drift's release key. \
                             Drift closes, installs it and opens again.",
                        );
                    }
                    Phase::Downloading { got, total } => {
                        ui.heading(format!("Downloading Drift {}", release.version));
                        ui.add_space(4.0);
                        let bar = match total {
                            Some(t) if *t > 0 => egui::ProgressBar::new(*got as f32 / *t as f32)
                                .text(format!("{} of {}", mb(*got), mb(*t))),
                            _ => egui::ProgressBar::new(0.0).text(mb(*got)).animate(true),
                        };
                        ui.add(bar);
                    }
                    _ => {
                        ui.heading(format!("Drift {} is available", release.version));
                        ui.label(
                            egui::RichText::new(format!("You have {}.", env!("CARGO_PKG_VERSION")))
                                .weak(),
                        );
                        if !release.notes.trim().is_empty() {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(release.notes.trim()).small());
                                });
                        }
                        if !installable {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "This copy of Drift can't update itself (it wasn't \
                                     installed with the installer): download it from the \
                                     release page.",
                                )
                                .small()
                                .weak(),
                            );
                        }
                    }
                }
                if !release.page.is_empty() {
                    ui.hyperlink_to("Release page", &release.page);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let accent = theme::accent(ui.visuals().dark_mode);
                    let primary = |text: &str| {
                        egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
                            .fill(accent)
                    };
                    match &self.updates.phase {
                        Phase::Ready(_) => {
                            if ui.add(primary("Restart and update")).clicked() {
                                action = Some(NoticeAction::Install);
                            }
                            if ui.button("Later").clicked() {
                                action = Some(NoticeAction::Later);
                            }
                        }
                        Phase::Downloading { .. } => {
                            if ui.button("Cancel").clicked() {
                                action = Some(NoticeAction::Cancel);
                            }
                            if ui.button("Hide").clicked() {
                                action = Some(NoticeAction::Later);
                            }
                        }
                        _ => {
                            if installable {
                                if ui.add(primary("Update now")).clicked() {
                                    action = Some(NoticeAction::Update);
                                }
                            } else if ui.add(primary("Download page")).clicked() {
                                action = Some(NoticeAction::Page);
                            }
                            if ui.button("Later").clicked() {
                                action = Some(NoticeAction::Later);
                            }
                            if ui.button("Skip this version").clicked() {
                                action = Some(NoticeAction::Skip);
                            }
                        }
                    }
                });
            });
        let Some(action) = action else {
            return;
        };
        let u = &mut self.updates;
        match action {
            NoticeAction::Update => {
                u.install_when_ready = true;
                self.start_download(ctx);
            }
            NoticeAction::Install => self.request(Pending::Update),
            NoticeAction::Page => {
                let url = if release.page.is_empty() {
                    update::DOWNLOAD_PAGE
                } else {
                    &release.page
                };
                ctx.open_url(egui::OpenUrl::new_tab(url));
                u.show = false;
            }
            NoticeAction::Later => u.show = false,
            NoticeAction::Cancel => u.cancel.store(true, Ordering::Relaxed),
            NoticeAction::Skip => {
                u.skipped = Some(release.version);
                u.release = None;
                u.phase = Phase::Idle;
                u.show = false;
            }
        }
    }

    /// Preferences → Updates.
    pub(crate) fn updates_section(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Updates");
        ui.checkbox(
            &mut self.updates.auto_check,
            "Check for updates when Drift starts",
        );
        ui.label(
            egui::RichText::new(
                "Drift asks GitHub (api.github.com) for the list of Drift releases. Nothing \
                 else is sent: not your drawings, and no account or ID. Updates are installed \
                 only when you say so, and only if they are signed with Drift's release key.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            let busy = self.updates.busy();
            if ui
                .add_enabled(!busy, egui::Button::new("Check now"))
                .clicked()
            {
                let ctx = ui.ctx().clone();
                self.check_for_updates(&ctx, true);
            }
            let note = match (&self.updates.phase, &self.updates.release) {
                (Phase::Checking, _) => "Checking…".to_owned(),
                (Phase::Available | Phase::Downloading { .. }, Some(r)) => {
                    format!("Drift {} is available", r.version)
                }
                (Phase::Ready(_), Some(r)) => format!("Drift {} is ready to install", r.version),
                _ => format!("This is Drift {}", env!("CARGO_PKG_VERSION")),
            };
            ui.label(egui::RichText::new(note).weak());
        });
        if let Some(skipped) = self.updates.skipped.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Skipping {skipped}")).weak());
                if ui.small_button("Offer it again").clicked() {
                    self.updates.skipped = None;
                }
            });
        }
        if !self.updates.install.can_update() {
            ui.label(
                egui::RichText::new(
                    "This copy wasn't installed with the installer, so an update is downloaded \
                     from the release page by hand.",
                )
                .small()
                .weak(),
            );
        }
    }
}

enum NoticeAction {
    Update,
    Install,
    Page,
    Later,
    Cancel,
    Skip,
}

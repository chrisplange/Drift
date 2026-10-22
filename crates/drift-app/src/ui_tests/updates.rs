//! The update notice, its buttons, and Preferences → Updates. No network: the worker's
//! messages are handed to the app directly, and nothing is installed.

use egui_kittest::kittest::Queryable as _;

use super::{harness, save_png};
use crate::app::update_ui::{Msg, Phase};
use crate::update::install::{Install, Plan};
use crate::update::{Asset, Platform, Release};

fn newer() -> Release {
    let version = semver::Version::parse("99.0.0-alpha.1").unwrap();
    let assets = [Platform::Windows, Platform::MacOs]
        .map(|p| p.asset_name(&version))
        .into_iter()
        .chain(["SHA256SUMS.txt".into(), "SHA256SUMS.txt.minisig".into()])
        .map(|name| Asset {
            url: format!("https://dl.example/{name}"),
            name,
            size: 10,
        })
        .collect();
    Release {
        version,
        title: "Drift 99.0.0-alpha.1".into(),
        notes: "- Faster solver\n- New hatch patterns".into(),
        page: "https://github.com/chrisplange/drift-releases/releases/tag/v99.0.0-alpha.1".into(),
        prerelease: true,
        assets,
    }
}

/// How an installed copy looks on this system.
fn installed() -> Install {
    match Platform::current() {
        Some(Platform::MacOs) => Install::Bundle("/Applications/Drift.app".into()),
        _ => Install::Installer,
    }
}

#[test]
fn a_newer_release_is_offered_and_can_be_put_off_or_skipped() {
    let Some(mut h) = harness() else { return };
    h.state_mut().updates.install = installed();
    h.state_mut()
        .update_message(Msg::Checked(Ok(Some(newer()))), 0.0);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Drift 99.0.0-alpha.1 is available")
            .is_some()
    );
    assert!(h.query_by_label("Skip this version").is_some());
    if Platform::current().is_some() {
        assert!(h.query_by_label("Update now").is_some());
        save_png("drift_ui_update_notice.png", &mut h);
    } else {
        // No installer for this system: the release page instead.
        assert!(h.query_by_label("Download page").is_some());
    }

    // Later hides it for now.
    h.get_by_label("Later").click();
    h.run();
    assert!(!h.state().updates.show);
    assert!(
        h.query_by_label("Drift 99.0.0-alpha.1 is available")
            .is_none()
    );

    // Skip: not offered again at start, and remembered.
    h.state_mut().updates.show = true;
    h.run();
    h.get_by_label("Skip this version").click();
    h.run();
    let skipped = h.state().updates.skipped.clone();
    assert_eq!(
        skipped.map(|v| v.to_string()).as_deref(),
        Some("99.0.0-alpha.1")
    );
    h.state_mut()
        .update_message(Msg::Checked(Ok(Some(newer()))), 0.0);
    h.run();
    assert!(!h.state().updates.show);
    assert!(
        h.query_by_label("Drift 99.0.0-alpha.1 is available")
            .is_none()
    );

    let prefs = h.state().view_prefs();
    assert!(prefs.contains("upskip=99.0.0-alpha.1"), "{prefs}");
    assert!(prefs.contains("upchk=1"), "{prefs}");
    let Some(mut fresh) = harness() else { return };
    fresh
        .state_mut()
        .apply_view_prefs(&prefs.replace("upchk=1", "upchk=0"));
    assert_eq!(
        fresh.state().updates.skipped,
        h.state().updates.skipped,
        "the skipped version is kept"
    );
    assert!(!fresh.state().updates.auto_check);
}

#[test]
fn a_copy_that_was_not_installed_points_to_the_download_page() {
    let Some(mut h) = harness() else { return };
    h.state_mut().updates.install = Install::Manual;
    h.state_mut()
        .update_message(Msg::Checked(Ok(Some(newer()))), 0.0);
    h.run();
    h.run();
    assert!(h.query_by_label("Update now").is_none());
    assert!(h.query_by_label("Download page").is_some());
}

#[test]
fn failures_and_no_update_are_status_messages() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .update_message(Msg::Checked(Err("no connection to GitHub".into())), 0.0);
    h.run();
    let status = h.state().status.as_ref().map(|s| (s.text.clone(), s.error));
    assert_eq!(
        status,
        Some((
            "Couldn't check for updates: no connection to GitHub".into(),
            false
        ))
    );
    assert!(!h.state().updates.show);
    assert!(matches!(h.state().updates.phase, Phase::Idle));
}

#[test]
fn installing_asks_about_unsaved_work_first() {
    let Some(mut h) = harness() else { return };
    h.state_mut().updates.install = installed();
    h.state_mut()
        .update_message(Msg::Checked(Ok(Some(newer()))), 0.0);
    // A verified download is ready (the plan is never run: tests don't exit the app).
    let plan = Plan::Windows {
        installer: "Drift-99.0.0-alpha.1-windows-x64-setup.exe".into(),
    };
    h.state_mut()
        .update_message(Msg::Prepared(Ok(plan.clone())), 0.0);
    h.run();
    h.run();
    assert!(
        h.query_by_label("Drift 99.0.0-alpha.1 is ready to install")
            .is_some()
    );
    // An unsaved change.
    h.state_mut().perform(crate::Pending::Example);
    assert!(h.state().is_dirty());
    h.get_by_label("Restart and update").click();
    h.run();
    h.run();
    assert_eq!(h.state().pending, Some(crate::Pending::Update));
    save_png("drift_ui_update_prompt.png", &mut h);
    // Cancel keeps the download for later.
    h.get_by_label("Cancel").click();
    h.run();
    assert!(!h.state().quit_confirmed);
    assert!(h.state().updates.on_exit.is_none());
    assert!(matches!(h.state().updates.phase, Phase::Ready(_)));
    // Don't save: Drift closes and the installer is set to run as it goes.
    h.get_by_label("Restart and update").click();
    h.run();
    h.get_by_label("Don't save").click();
    h.run();
    assert!(h.state().quit_confirmed);
    assert_eq!(h.state().updates.on_exit, Some(plan));
}

#[test]
fn preferences_have_an_updates_section() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    h.get_by_label("Updates & Labs").click();
    h.run();
    h.run();
    assert!(
        h.query_by_label("Check for updates when Drift starts")
            .is_some()
    );
    assert!(h.query_by_label("Check now").is_some());
    // On by default.
    assert!(h.state().updates.auto_check);
}

#[test]
fn the_help_menu_checks_for_updates() {
    let Some(mut h) = harness() else { return };
    h.get_by_label("Help").click();
    h.run();
    assert!(h.query_by_label("Check for updates…").is_some());
}

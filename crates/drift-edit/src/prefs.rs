//! The preferences Drift keeps on this computer that editing needs: the rules new drawings
//! start with. The app writes them (in eframe's storage, `app.ron` in Drift's data folder);
//! the assistant's server reads them from there, apart from the app.

/// The preferences key of the view preferences string (grid, axes, snapping, thin lines,
/// colours, annotations, and the rules new drawings start with).
pub const KEY_VIEW: &str = "drift.view";

/// Read one of the rules new drawings start with from a saved preference (`key=value`).
/// False when `key` isn't one of them.
pub fn rules_pref(rules: &mut drift_core::DrawingRules, key: &str, value: &str) -> bool {
    let on = value == "1";
    match key {
        "rlayer" => rules.regions_by_layer = on,
        "rfill" => rules.regions_need_fill = on,
        "stack" => rules.stacked_fills = on,
        "dfollow" => rules.follow_divided_fills = on,
        "gaptol" => {
            if let Some(t) = value
                .parse::<f64>()
                .ok()
                .filter(|t| *t > 0.0 && t.is_finite())
            {
                rules.gap_tolerance = t;
            }
        }
        _ => return false,
    }
    true
}

/// The rules new drawings start with, from saved preferences (see `view_prefs`).
#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
fn rules_from_prefs(text: &str) -> drift_core::DrawingRules {
    let mut rules = drift_core::DrawingRules::default();
    for (key, value) in text.split(';').filter_map(|p| p.split_once('=')) {
        rules_pref(&mut rules, key, value);
    }
    rules
}

/// The rules Drift starts new drawings with on this computer, read from its saved settings
/// (for the assistant's server, which runs apart from the app). The defaults if there are
/// none.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "demo")))]
pub fn saved_new_rules() -> drift_core::DrawingRules {
    let text = drift_platform::storage_dir("Drift")
        .and_then(|dir| std::fs::read_to_string(dir.join("app.ron")).ok())
        .unwrap_or_default();
    // A RON map of strings: `"drift.view": "grid=1;…"` (the preferences hold no quotes).
    let key = format!("\"{KEY_VIEW}\"");
    let prefs = text
        .split_once(&key)
        .and_then(|(_, rest)| rest.trim_start().strip_prefix(':'))
        .and_then(|rest| rest.trim_start().strip_prefix('"'))
        .and_then(|rest| rest.split_once('"'))
        .map_or("", |(prefs, _)| prefs);
    rules_from_prefs(prefs)
}

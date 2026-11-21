//! What a new drawing starts with on this computer, beyond its rules: its units (Preferences →
//! New drawings use, #495) and its title block logo (#429). Kept in eframe's storage with the
//! app's other settings.

use drift_core::{Document, Units};

/// Which units new drawings start in ("metric" or "imperial").
const KEY_UNITS: &str = "drift.new_units";
/// The title block logo new drawings start with, as SVG text ("" for none).
const KEY_LOGO: &str = "drift.default_logo";

/// The units a new drawing starts in: millimetres, or architectural feet and inches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewUnits {
    #[default]
    Metric,
    Imperial,
}

impl NewUnits {
    pub(crate) fn units(self) -> Units {
        match self {
            NewUnits::Metric => Units::metric(),
            NewUnits::Imperial => Units::architectural(),
        }
    }

    /// Metric or imperial, as `units` are.
    pub(crate) fn of(units: Units) -> Self {
        if units.length.is_metric() {
            NewUnits::Metric
        } else {
            NewUnits::Imperial
        }
    }

    fn key(self) -> &'static str {
        match self {
            NewUnits::Metric => "metric",
            NewUnits::Imperial => "imperial",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "metric" => Some(NewUnits::Metric),
            "imperial" => Some(NewUnits::Imperial),
            _ => None,
        }
    }

    /// By a locale name (`en_US.UTF-8`, `en-US`): imperial for the United States, metric
    /// otherwise.
    pub(crate) fn from_locale(name: &str) -> Self {
        let name = name.trim().to_ascii_lowercase().replace('-', "_");
        let tag = name.split(['.', '@']).next().unwrap_or_default();
        if tag.split('_').nth(1) == Some("us") {
            NewUnits::Imperial
        } else {
            NewUnits::Metric
        }
    }

    /// The first run's choice (only asked when no choice is kept): by the system's settings.
    /// Windows: the Region setting's measurement system, else its locale; macOS: the
    /// measurement units, else the locale; then the first of `LC_ALL`, `LC_MEASUREMENT`,
    /// `LC_MESSAGES` and `LANG` that is set. Metric when nothing says (and in the browser).
    /// Tests always start metric, whatever machine they run on.
    fn by_locale() -> Self {
        if cfg!(test) {
            return NewUnits::Metric;
        }
        Self::by_system()
            .or_else(Self::by_env)
            .unwrap_or(NewUnits::Metric)
    }

    fn by_env() -> Option<Self> {
        ["LC_ALL", "LC_MEASUREMENT", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.trim().is_empty())
            .map(|v| Self::from_locale(&v))
    }

    /// Windows: `reg query "HKCU\Control Panel\International"`, for `iMeasure` (0 metric, 1
    /// US) or else `LocaleName`.
    #[cfg(windows)]
    fn by_system() -> Option<Self> {
        use std::os::windows::process::CommandExt as _;
        // Without a console window flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Windows' own reg.exe, not whichever `reg` comes first on the search path.
        let reg = std::path::PathBuf::from(
            std::env::var_os("SystemRoot")
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| r"C:\Windows".into()),
        )
        .join("System32")
        .join("reg.exe");
        let query = |name: &str| {
            std::process::Command::new(&reg)
                .args(["query", r"HKCU\Control Panel\International", "/v", name])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| reg_value(&String::from_utf8_lossy(&o.stdout), name))
        };
        query("iMeasure")
            .and_then(|v| from_imeasure(&v))
            .or_else(|| query("LocaleName").map(|v| Self::from_locale(&v)))
    }

    /// macOS: `defaults read -g AppleMeasurementUnits` (Inches or Centimeters), or else
    /// `AppleLocale`.
    #[cfg(target_os = "macos")]
    fn by_system() -> Option<Self> {
        let read = |name: &str| {
            std::process::Command::new("defaults")
                .args(["read", "-g", name])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        read("AppleMeasurementUnits")
            .and_then(|v| from_apple_units(&v))
            .or_else(|| read("AppleLocale").map(|v| Self::from_locale(&v)))
    }

    /// Elsewhere the locale's environment variables say (the browser has none: metric).
    #[cfg(not(any(windows, target_os = "macos")))]
    fn by_system() -> Option<Self> {
        None
    }
}

/// The value of `name` in `reg query` output (a line `    name    REG_SZ    value`).
#[cfg(any(windows, test))]
fn reg_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case(name) || !words.next()?.starts_with("REG_") {
            return None;
        }
        let value = words.collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    })
}

/// Windows' `iMeasure`: 0 metric, 1 US.
#[cfg(any(windows, test))]
fn from_imeasure(value: &str) -> Option<NewUnits> {
    match value.trim() {
        "0" => Some(NewUnits::Metric),
        "1" => Some(NewUnits::Imperial),
        _ => None,
    }
}

/// macOS's `AppleMeasurementUnits`.
#[cfg(any(target_os = "macos", test))]
fn from_apple_units(value: &str) -> Option<NewUnits> {
    match value.trim() {
        "Inches" => Some(NewUnits::Imperial),
        "Centimeters" => Some(NewUnits::Metric),
        _ => None,
    }
}

/// What new drawings start with (see the module's note).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewDrawing {
    pub(crate) units: NewUnits,
    /// The title block logo, as SVG text ("" for none): the last one chosen for a drawing.
    /// Removing a drawing's logo leaves it; Preferences clears or replaces it.
    pub(crate) logo: String,
    /// The logos chosen for drawings, newest last, and those whose choice was undone: undo
    /// and redo of the choice take the default back and forth with it.
    chosen: Vec<LogoChoice>,
    undone: Vec<LogoChoice>,
}

/// A logo chosen for a drawing, and the default it replaced.
#[derive(Clone, Debug, PartialEq)]
struct LogoChoice {
    before: String,
    chosen: String,
}

impl NewDrawing {
    /// As saved; a first run's units follow the system's locale.
    pub(crate) fn load(storage: &dyn eframe::Storage) -> Self {
        let units = storage
            .get_string(KEY_UNITS)
            .and_then(|v| NewUnits::parse(&v))
            .unwrap_or_else(NewUnits::by_locale);
        // (One this Drift can't read is dropped rather than given to every new drawing.)
        let logo = storage
            .get_string(KEY_LOGO)
            .filter(|svg| !svg.is_empty() && drift_draft::logo::parse(svg).is_ok())
            .unwrap_or_default();
        Self {
            units,
            logo,
            ..Self::default()
        }
    }

    /// A logo chosen for a drawing: new drawings start with it too. One the drawing `had`
    /// already makes no undo step, so there is no choice to undo either.
    pub(crate) fn choose_logo(&mut self, svg: &str, had: bool) {
        let before = std::mem::replace(&mut self.logo, svg.to_owned());
        if had {
            return;
        }
        self.chosen.push(LogoChoice {
            before,
            chosen: svg.to_owned(),
        });
        self.undone.clear();
    }

    /// The default set in Preferences (cleared, or the open drawing's).
    pub(crate) fn set_logo(&mut self, svg: String) {
        self.logo = svg;
        self.chosen.clear();
        self.undone.clear();
    }

    /// An undo (or redo) changed the drawing's logo `from` one `to` another. Undoing a
    /// choice puts back the default it replaced, if the default is still the logo undone;
    /// redoing it makes it the default again, if that is still the one it replaced.
    fn logo_stepped(&mut self, from: &str, to: &str, undo: bool) {
        if undo {
            if let Some(c) = self.chosen.pop_if(|c| c.chosen == from) {
                if self.logo == c.chosen {
                    self.logo = c.before.clone();
                }
                self.undone.push(c);
            }
        } else if let Some(c) = self.undone.pop_if(|c| c.chosen == to) {
            if self.logo == c.before {
                self.logo = c.chosen.clone();
            }
            self.chosen.push(c);
        }
    }

    pub(crate) fn save(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(KEY_UNITS, self.units.key().into());
        storage.set_string(KEY_LOGO, self.logo.clone());
    }

    /// Give `doc`, a new drawing, what new drawings start with.
    pub(crate) fn apply(&self, doc: &mut Document) {
        doc.set_units(self.units.units());
        doc.set_logo(self.logo.clone());
    }
}

impl crate::DriftApp {
    /// Undo (or redo) a step; one that changed the logo chosen for the drawing takes the
    /// default with it (see [`NewDrawing::logo_stepped`]).
    pub(crate) fn step_history(&mut self, undo: bool) {
        let before = self.drawing.editor.doc.logo().to_owned();
        if undo {
            self.drawing.editor.undo();
        } else {
            self.drawing.editor.redo();
        }
        let after = self.drawing.editor.doc.logo();
        if after != before {
            let after = after.to_owned();
            self.new_drawing.logo_stepped(&before, &after, undo);
        }
    }

    /// Whether the drawing open now is a new one nothing has been done to: started as new
    /// (not opened or restored), no file, nothing to save, nothing in it.
    pub(crate) fn untouched_new_drawing(&self) -> bool {
        let doc = &self.drawing.editor.doc;
        self.drawing_from_new
            && self.drawing.file.path.is_none()
            && !self.is_dirty()
            && doc.is_empty()
            && doc.texts().next().is_none()
            && doc.underlays().next().is_none()
    }

    /// A drawing kept from last time was just restored. The session doesn't say whether it was
    /// started as a new one: one still named Untitled, with no file, nothing to save and
    /// nothing in it, was (an opened file keeps its own name, also in the browser, where it
    /// has no path). That one starts as new drawings do now.
    pub(crate) fn kept_blank_restored(&mut self) {
        self.drawing_from_new = self.drawing.file.name == crate::UNTITLED;
        self.refresh_untouched_drawing();
    }

    /// Give the drawing open now what new drawings start with, if it is an untouched new one
    /// (it stays clean): after the choice changes, and for a blank drawing restored at start.
    pub(crate) fn refresh_untouched_drawing(&mut self) {
        if !self.untouched_new_drawing() {
            return;
        }
        self.new_drawing.apply(&mut self.drawing.editor.doc);
        self.drawing.file.clean_revision = Some(self.drawing.editor.doc.revision());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_locales_are_imperial() {
        for name in [
            "en_US.UTF-8",
            "en_US",
            "en-US",
            "EN_us.utf8",
            "en_US@euro",
            "es_US",
        ] {
            assert_eq!(NewUnits::from_locale(name), NewUnits::Imperial, "{name}");
        }
        for name in [
            "en_GB.UTF-8",
            "de_DE",
            "C",
            "POSIX",
            "",
            "en",
            "en_USA",
            "us",
        ] {
            assert_eq!(NewUnits::from_locale(name), NewUnits::Metric, "{name}");
        }
    }

    /// Windows' `reg query` output, as it prints it.
    #[test]
    fn windows_region_settings_are_read() {
        let out = |name: &str, kind: &str, value: &str| {
            format!(
                "\r\nHKEY_CURRENT_USER\\Control Panel\\International\r\n    {name}    {kind}    \
                 {value}\r\n\r\n"
            )
        };
        let us = out("iMeasure", "REG_SZ", "1");
        assert_eq!(reg_value(&us, "iMeasure").as_deref(), Some("1"));
        assert_eq!(
            reg_value(&us, "iMeasure").and_then(|v| from_imeasure(&v)),
            Some(NewUnits::Imperial)
        );
        let metric = out("iMeasure", "REG_SZ", "0");
        assert_eq!(
            reg_value(&metric, "iMeasure").and_then(|v| from_imeasure(&v)),
            Some(NewUnits::Metric)
        );
        assert_eq!(from_imeasure("2"), None);
        let locale = out("LocaleName", "REG_SZ", "en-US");
        assert_eq!(
            reg_value(&locale, "LocaleName").map(|v| NewUnits::from_locale(&v)),
            Some(NewUnits::Imperial)
        );
        let locale = out("LocaleName", "REG_SZ", "en-GB");
        assert_eq!(
            reg_value(&locale, "LocaleName").map(|v| NewUnits::from_locale(&v)),
            Some(NewUnits::Metric)
        );
        // Another value, an empty one, or an error message: nothing read.
        assert_eq!(reg_value(&locale, "iMeasure"), None);
        assert_eq!(reg_value(&out("iMeasure", "REG_SZ", ""), "iMeasure"), None);
        assert_eq!(
            reg_value(
                "ERROR: The system was unable to find the specified registry key or value.",
                "iMeasure"
            ),
            None
        );
    }

    /// macOS's `defaults read -g` output.
    #[test]
    fn macos_measurement_units_are_read() {
        assert_eq!(from_apple_units("Inches\n"), Some(NewUnits::Imperial));
        assert_eq!(from_apple_units("Centimeters\n"), Some(NewUnits::Metric));
        assert_eq!(from_apple_units(""), None);
        assert_eq!(NewUnits::from_locale("en_US\n"), NewUnits::Imperial);
        assert_eq!(NewUnits::from_locale("fr_FR"), NewUnits::Metric);
    }

    #[test]
    fn units_keys_round_trip() {
        for u in [NewUnits::Metric, NewUnits::Imperial] {
            assert_eq!(NewUnits::parse(u.key()), Some(u));
            assert_eq!(NewUnits::of(u.units()), u);
        }
        assert_eq!(NewUnits::parse("furlongs"), None);
    }
}

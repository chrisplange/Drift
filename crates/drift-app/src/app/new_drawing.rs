//! What a new drawing starts with on this computer, beyond its rules: its units (Preferences →
//! New drawings use, #495). Kept in eframe's storage with the app's other settings.

use drift_core::{Document, Units};

/// Which units new drawings start in ("metric" or "imperial").
const KEY_UNITS: &str = "drift.new_units";

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

    /// The first run's choice: by the system's locale, from the first of `LC_ALL`,
    /// `LC_MEASUREMENT`, `LC_MESSAGES` and `LANG` that is set; metric when none is (Windows
    /// and the browser set none). Tests always start metric, whatever machine they run on.
    fn by_locale() -> Self {
        if cfg!(test) {
            return NewUnits::Metric;
        }
        ["LC_ALL", "LC_MEASUREMENT", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.trim().is_empty())
            .map_or(NewUnits::Metric, |v| Self::from_locale(&v))
    }
}

/// What new drawings start with (see the module's note).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewDrawing {
    pub(crate) units: NewUnits,
}

impl NewDrawing {
    /// As saved; a first run's units follow the system's locale.
    pub(crate) fn load(storage: &dyn eframe::Storage) -> Self {
        let units = storage
            .get_string(KEY_UNITS)
            .and_then(|v| NewUnits::parse(&v))
            .unwrap_or_else(NewUnits::by_locale);
        Self { units }
    }

    pub(crate) fn save(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(KEY_UNITS, self.units.key().into());
    }

    /// Give `doc`, a new drawing, what new drawings start with.
    pub(crate) fn apply(&self, doc: &mut Document) {
        doc.set_units(self.units.units());
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

    #[test]
    fn units_keys_round_trip() {
        for u in [NewUnits::Metric, NewUnits::Imperial] {
            assert_eq!(NewUnits::parse(u.key()), Some(u));
            assert_eq!(NewUnits::of(u.units()), u);
        }
        assert_eq!(NewUnits::parse("furlongs"), None);
    }
}

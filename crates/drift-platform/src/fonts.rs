//! Fonts installed on the computer, for sketch text. On the desktop the system's fonts are
//! found on first use (a scan of the font folders, kept for the session); in the browser
//! there are none to read, and text keeps to the built-in fonts.

/// The families installed, sorted by name. Empty in the browser.
pub fn families() -> Vec<String> {
    imp::families()
}

/// The regular face of a family: the font file's bytes and the face's index in it (font
/// collections hold several). `None` if the family isn't installed.
pub fn load(family: &str) -> Option<(Vec<u8>, u32)> {
    imp::load(family)
}

/// The bold face of a family, if it has one of its own.
pub fn load_bold(family: &str) -> Option<(Vec<u8>, u32)> {
    imp::load_bold(family)
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::sync::OnceLock;

    use fontdb::{Database, Family, Query, Stretch, Style, Weight};

    fn database() -> &'static Database {
        static DB: OnceLock<Database> = OnceLock::new();
        DB.get_or_init(|| {
            let mut db = Database::new();
            db.load_system_fonts();
            db
        })
    }

    pub fn families() -> Vec<String> {
        let mut names: Vec<String> = database()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            // Hidden system fonts (macOS names them with a leading dot).
            .filter(|name| !name.starts_with('.'))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    pub fn load(family: &str) -> Option<(Vec<u8>, u32)> {
        let db = database();
        let id = db.query(&Query {
            families: &[Family::Name(family)],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: Style::Normal,
        })?;
        db.with_face_data(id, |data, index| (data.to_vec(), index))
    }

    pub fn load_bold(family: &str) -> Option<(Vec<u8>, u32)> {
        let db = database();
        let id = db.query(&Query {
            families: &[Family::Name(family)],
            weight: Weight::BOLD,
            stretch: Stretch::Normal,
            style: Style::Normal,
        })?;
        // The nearest match may be the regular face again.
        if db.face(id)?.weight.0 < 600 {
            return None;
        }
        db.with_face_data(id, |data, index| (data.to_vec(), index))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    pub fn families() -> Vec<String> {
        Vec::new()
    }

    pub fn load(_family: &str) -> Option<(Vec<u8>, u32)> {
        None
    }

    pub fn load_bold(_family: &str) -> Option<(Vec<u8>, u32)> {
        None
    }
}

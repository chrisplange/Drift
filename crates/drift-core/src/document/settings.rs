//! Drawing-wide settings carried in the file: patterns, logo, rules, project, fonts and
//! the drafting style.

use super::*;

impl Document {
    /// Imported hatch patterns, in `.pat` format.
    pub fn custom_patterns(&self) -> &str {
        &self.patterns
    }

    pub fn set_custom_patterns(&mut self, pat: String) {
        if *self.patterns != pat {
            self.touch_key(Key::Patterns);
            self.patterns = pat.into();
        }
    }

    /// The logo drawn in the sheets' title blocks, as SVG text ("" for none).
    pub fn logo(&self) -> &str {
        &self.logo
    }

    pub fn set_logo(&mut self, svg: String) {
        if *self.logo != svg {
            self.touch_key(Key::Logo);
            self.logo = svg.into();
        }
    }

    /// How the drawing's closed areas and fills behave (saved with it).
    pub fn rules(&self) -> crate::DrawingRules {
        self.rules
    }

    pub fn set_rules(&mut self, rules: crate::DrawingRules) {
        if self.rules != rules {
            self.touch_key(Key::Rules);
            self.rules = rules;
        }
    }

    /// The project the sheets are for: their shared title block fields.
    pub fn project(&self) -> &crate::Project {
        &self.project
    }

    pub fn set_project(&mut self, project: crate::Project) {
        if self.project != project {
            self.touch_key(Key::Project);
            self.project = project;
        }
    }

    /// The fonts kept in the drawing, and the one its annotations are set in.
    pub fn fonts(&self) -> &crate::fonts::FontSet {
        &self.fonts
    }

    pub fn set_fonts(&mut self, fonts: crate::fonts::FontSet) {
        if *self.fonts != fonts {
            self.touch_key(Key::Fonts);
            self.fonts = fonts.into();
        }
    }

    /// The families the drawing uses: its annotations' font, and its texts' system fonts.
    pub fn fonts_in_use(&self) -> BTreeSet<String> {
        let mut used: BTreeSet<String> = self.fonts.drawing.iter().cloned().collect();
        for text in self.texts.values() {
            if let crate::texts::TextFont::System(family) = &text.font {
                used.insert(family.clone());
            }
        }
        used
    }

    pub fn draft_style(&self) -> DraftStyle {
        self.draft
    }

    pub fn set_draft_style(&mut self, style: DraftStyle) {
        if self.draft != style {
            self.touch_key(Key::Draft);
            self.draft = style;
        }
    }
}

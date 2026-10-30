//! Fonts in the app: keeping the fonts a drawing uses inside it, the drawing's own font for
//! dimensions, notes and sheets, and making that font known to the screen's text renderer.

use std::cell::Cell;
use std::rc::Rc;

use drift_draft::font::Face;
use eframe::egui::{self, FontFamily, FontId};

use crate::Editor;

thread_local! {
    /// Whether the screen's text renderer has the drawing's font (and a bold face for it),
    /// under the family names below.
    static ON_SCREEN: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

const FAMILY: &str = "drawing";
const FAMILY_BOLD: &str = "drawing-bold";

/// The font for annotation and sheet text on screen at `size`: the drawing's own once the
/// renderer has it, else the interface font.
/// Whether a font family is a bold one (the drawing's bold face, or the interface's).
pub fn is_bold(family: &FontFamily) -> bool {
    *family == FontFamily::Name(FAMILY_BOLD.into()) || *family == crate::theme::bold()
}

pub fn drafting_font(size: f32, bold: bool) -> FontId {
    let (ready, has_bold) = ON_SCREEN.with(Cell::get);
    let family = match (ready, bold) {
        (true, true) if has_bold => FontFamily::Name(FAMILY_BOLD.into()),
        (true, _) => FontFamily::Name(FAMILY.into()),
        (false, true) => crate::theme::bold(),
        (false, false) => FontFamily::Proportional,
    };
    FontId::new(size, family)
}

/// Where the screen's text renderer is with the drawing's font.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum ScreenFont {
    /// The interface font (no drawing font).
    #[default]
    Standard,
    /// Given to the renderer this frame: usable from the next.
    Loading(usize),
    Ready(usize),
}

/// Bring the screen's text renderer in step with the drawing's font, and set it as the font
/// text is measured in. Call once a frame, before anything is laid out.
pub(crate) fn sync_screen(ctx: &egui::Context, editor: &Editor, state: &mut ScreenFont) {
    let font = editor.drawing_font();
    drift_draft::font::set_drawing_font(font.clone());
    let Some(font) = font else {
        *state = ScreenFont::Standard;
        ON_SCREEN.with(|s| s.set((false, false)));
        return;
    };
    let id = Rc::as_ptr(&font) as usize;
    match state {
        ScreenFont::Ready(have) if *have == id => {}
        // Handed over last frame: the renderer has it now.
        ScreenFont::Loading(have) if *have == id => {
            *state = ScreenFont::Ready(id);
            ON_SCREEN.with(|s| s.set((true, font.bold.is_some())));
        }
        _ => {
            ON_SCREEN.with(|s| s.set((false, false)));
            // The interface's fonts again, plus the drawing's with them behind it for the
            // characters it lacks.
            let mut fonts = crate::theme::font_definitions();
            let mut insert = |face: &Face, family: &str, behind: FontFamily| {
                let mut data = egui::FontData::from_owned(face.data.to_vec());
                data.index = face.index;
                fonts.font_data.insert(family.to_owned(), data.into());
                let mut list = vec![family.to_owned()];
                list.extend(fonts.families.get(&behind).cloned().unwrap_or_default());
                fonts.families.insert(FontFamily::Name(family.into()), list);
            };
            insert(&font.regular, FAMILY, FontFamily::Proportional);
            if let Some(bold) = &font.bold {
                insert(bold, FAMILY_BOLD, crate::theme::bold());
            }
            ctx.set_fonts(fonts);
            *state = ScreenFont::Loading(id);
            ctx.request_repaint();
        }
    }
}

/// Forget that the renderer has a drawing font (a new window starts without one).
pub(crate) fn reset_screen() {
    ON_SCREEN.with(|s| s.set((false, false)));
    drift_draft::font::set_drawing_font(None);
}

/// Canvas colours, as sRGB bytes (unmultiplied alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The drawing's background: the one place it is set. The app's theme reads it from
    /// here for whatever it paints to match the canvas.
    pub background: [u8; 4],
    /// The desk a sheet's paper lies on (painted by the app, not by this crate).
    pub desk: [u8; 4],
    pub grid_minor: [u8; 4],
    pub grid_major: [u8; 4],
    pub axis_x: [u8; 4],
    pub axis_y: [u8; 4],
    pub solid: [u8; 4],
    pub construction: [u8; 4],
    pub center: [u8; 4],
    pub point: [u8; 4],
    pub hover: [u8; 4],
    /// An end point under the pointer: a bright dot, ringed in `selected`.
    pub point_hover: [u8; 4],
    pub selected: [u8; 4],
    /// The selection's active element, drawn apart from the rest of the selection.
    pub active: [u8; 4],
    pub preview: [u8; 4],
    /// Geometry that can still move.
    pub under: [u8; 4],
    /// Geometry in conflicting constraints.
    pub over: [u8; 4],
}

impl Palette {
    /// The canvas's colours in the dark or the light appearance.
    pub fn themed(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn dark() -> Self {
        Self {
            background: [0x18, 0x18, 0x1a, 0xff],
            desk: [0x1e, 0x1e, 0x21, 0xff],
            grid_minor: [0x20, 0x20, 0x23, 0xff],
            grid_major: [0x2a, 0x2a, 0x2e, 0xff],
            axis_x: [0xc2, 0x4a, 0x4a, 0xb0],
            axis_y: [0x4a, 0xa8, 0x5c, 0xb0],
            solid: [0xe6, 0xe8, 0xeb, 0xff],
            construction: [0x8e, 0x9a, 0xb0, 0xff],
            center: [0xa0, 0xa6, 0xad, 0xff],
            point: [0xf2, 0xb5, 0x3c, 0xff],
            hover: [0xff, 0xd2, 0x7a, 0xff],
            point_hover: [0xff, 0xff, 0xff, 0xff],
            selected: [0xf5, 0xa6, 0x23, 0xff],
            active: [0xfb, 0xc9, 0x2b, 0xff],
            preview: [0x5e, 0xd3, 0xa3, 0xff],
            under: [0x5f, 0xa8, 0xf5, 0xff],
            over: [0xff, 0x5d, 0x5d, 0xff],
        }
    }

    pub fn light() -> Self {
        Self {
            background: [0xfb, 0xfb, 0xfa, 0xff],
            desk: [0xd4, 0xd4, 0xd9, 0xff],
            grid_minor: [0xf0, 0xf0, 0xf1, 0xff],
            grid_major: [0xe0, 0xe0, 0xe3, 0xff],
            axis_x: [0xd0, 0x3a, 0x3a, 0x90],
            axis_y: [0x2e, 0x9a, 0x48, 0x90],
            solid: [0x1d, 0x21, 0x26, 0xff],
            construction: [0x6b, 0x76, 0x8a, 0xff],
            center: [0x5c, 0x63, 0x6b, 0xff],
            point: [0xc7, 0x7a, 0x00, 0xff],
            hover: [0xf0, 0xa8, 0x30, 0xff],
            point_hover: [0x1d, 0x21, 0x26, 0xff],
            selected: [0xe0, 0x86, 0x00, 0xff],
            active: [0xb8, 0x64, 0x00, 0xff],
            preview: [0x13, 0x9a, 0x6a, 0xff],
            under: [0x15, 0x65, 0xc0, 0xff],
            over: [0xd3, 0x2f, 0x2f, 0xff],
        }
    }
}

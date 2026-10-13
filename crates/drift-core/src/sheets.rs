//! Sheets: a paper size and a drawing scale, a view of the sketch, a title block and a
//! revision table.

use glam::DVec2;

/// Stable identifier of a sheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetId(pub u32);

impl std::fmt::Display for SheetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// A standard paper size (ISO A-series or ANSI), or a custom one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaperSize {
    A4,
    A3,
    A2,
    A1,
    A0,
    AnsiA,
    AnsiB,
    AnsiC,
    AnsiD,
    AnsiE,
    /// Width and height in mm (landscape: width ≥ height).
    Custom(f64, f64),
}

impl PaperSize {
    pub const STANDARD: [PaperSize; 10] = [
        PaperSize::A4,
        PaperSize::A3,
        PaperSize::A2,
        PaperSize::A1,
        PaperSize::A0,
        PaperSize::AnsiA,
        PaperSize::AnsiB,
        PaperSize::AnsiC,
        PaperSize::AnsiD,
        PaperSize::AnsiE,
    ];

    /// The long and short sides in mm.
    pub fn sides(self) -> (f64, f64) {
        const IN: f64 = 25.4;
        match self {
            PaperSize::A4 => (297.0, 210.0),
            PaperSize::A3 => (420.0, 297.0),
            PaperSize::A2 => (594.0, 420.0),
            PaperSize::A1 => (841.0, 594.0),
            PaperSize::A0 => (1189.0, 841.0),
            PaperSize::AnsiA => (11.0 * IN, 8.5 * IN),
            PaperSize::AnsiB => (17.0 * IN, 11.0 * IN),
            PaperSize::AnsiC => (22.0 * IN, 17.0 * IN),
            PaperSize::AnsiD => (34.0 * IN, 22.0 * IN),
            PaperSize::AnsiE => (44.0 * IN, 34.0 * IN),
            PaperSize::Custom(w, h) => (w.max(h), w.min(h)),
        }
    }

    pub fn label(self) -> String {
        match self {
            PaperSize::A4 => "A4".into(),
            PaperSize::A3 => "A3".into(),
            PaperSize::A2 => "A2".into(),
            PaperSize::A1 => "A1".into(),
            PaperSize::A0 => "A0".into(),
            PaperSize::AnsiA => "ANSI A (letter)".into(),
            PaperSize::AnsiB => "ANSI B (tabloid)".into(),
            PaperSize::AnsiC => "ANSI C".into(),
            PaperSize::AnsiD => "ANSI D".into(),
            PaperSize::AnsiE => "ANSI E".into(),
            PaperSize::Custom(w, h) => format!("{w} × {h} mm"),
        }
    }

    /// The name used in files.
    pub fn key(self) -> String {
        match self {
            PaperSize::A4 => "A4".into(),
            PaperSize::A3 => "A3".into(),
            PaperSize::A2 => "A2".into(),
            PaperSize::A1 => "A1".into(),
            PaperSize::A0 => "A0".into(),
            PaperSize::AnsiA => "ANSI-A".into(),
            PaperSize::AnsiB => "ANSI-B".into(),
            PaperSize::AnsiC => "ANSI-C".into(),
            PaperSize::AnsiD => "ANSI-D".into(),
            PaperSize::AnsiE => "ANSI-E".into(),
            PaperSize::Custom(..) => "custom".into(),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::STANDARD.into_iter().find(|p| p.key() == key)
    }

    pub fn is_ansi(self) -> bool {
        matches!(
            self,
            PaperSize::AnsiA
                | PaperSize::AnsiB
                | PaperSize::AnsiC
                | PaperSize::AnsiD
                | PaperSize::AnsiE
        )
    }
}

/// Which title block a sheet carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleBlock {
    /// Drift's own: a full-width strip along the bottom, set in the Swiss manner.
    #[default]
    Modern,
    /// ISO 7200: a 180 mm block in the bottom-right corner.
    Iso,
    /// ASME Y14.1: a block along the bottom-right with the drawing number and size.
    Ansi,
    None,
}

impl TitleBlock {
    pub const ALL: [TitleBlock; 4] = [
        TitleBlock::Modern,
        TitleBlock::Iso,
        TitleBlock::Ansi,
        TitleBlock::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TitleBlock::Modern => "Modern",
            TitleBlock::Iso => "ISO 7200",
            TitleBlock::Ansi => "ASME Y14.1",
            TitleBlock::None => "None",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TitleBlock::Modern => "modern",
            TitleBlock::Iso => "iso",
            TitleBlock::Ansi => "ansi",
            TitleBlock::None => "none",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// One row of a revision table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Revision {
    pub rev: String,
    pub date: String,
    pub description: String,
    pub by: String,
}

/// The title block's fields. Scale, sheet number and revision are filled in automatically.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleFields {
    pub title: String,
    pub number: String,
    pub author: String,
    pub checked: String,
    pub date: String,
    pub company: String,
    pub material: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub paper: PaperSize,
    pub portrait: bool,
    /// The drawing scale, 1 : `scale`.
    pub scale: f64,
    /// The world point at the centre of the drawing area.
    pub center: DVec2,
    pub title_block: TitleBlock,
    pub fields: TitleFields,
    pub revisions: Vec<Revision>,
    /// Show dimensions (ordinates included) on this sheet.
    pub dimensions: bool,
    /// Show notes and callouts on this sheet.
    pub notes: bool,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            paper: PaperSize::A3,
            portrait: false,
            scale: 1.0,
            center: DVec2::ZERO,
            title_block: TitleBlock::Modern,
            fields: TitleFields::default(),
            revisions: Vec::new(),
            dimensions: true,
            notes: true,
        }
    }

    /// Paper width and height in mm, oriented.
    pub fn size(&self) -> DVec2 {
        let (long, short) = self.paper.sides();
        if self.portrait {
            DVec2::new(short, long)
        } else {
            DVec2::new(long, short)
        }
    }

    /// The latest revision's name, if any.
    pub fn revision(&self) -> Option<&str> {
        self.revisions.last().map(|r| r.rev.as_str())
    }
}

/// Preferred drawing scales (1 : n), smallest first. `fit_scale` picks from these.
pub const PREFERRED_SCALES: [f64; 22] = [
    0.1, 0.2, 0.5, 1.0, 2.0, 2.5, 5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 200.0, 250.0,
    500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0,
];

/// The largest preferred scale (smallest `n`) at which `extent` (world mm) fits in `room`
/// (paper mm).
pub fn fit_scale(extent: DVec2, room: DVec2) -> f64 {
    PREFERRED_SCALES
        .into_iter()
        .find(|n| extent.x / n <= room.x && extent.y / n <= room.y)
        .unwrap_or(*PREFERRED_SCALES.last().expect("scales"))
}

/// A scale as it is written on a drawing: `1:50`, `2:1`.
pub fn scale_label(n: f64) -> String {
    let trim = |v: f64| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    if n < 1.0 {
        format!("{}:1", trim(1.0 / n))
    } else {
        format!("1:{}", trim(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_scales_and_labels() {
        let mut s = Sheet::new("Sheet 1");
        assert_eq!(s.size(), DVec2::new(420.0, 297.0));
        s.portrait = true;
        assert_eq!(s.size(), DVec2::new(297.0, 420.0));
        assert!((PaperSize::AnsiB.sides().0 - 431.8).abs() < 1e-9);
        assert_eq!(PaperSize::parse("ANSI-D"), Some(PaperSize::AnsiD));
        assert_eq!(scale_label(50.0), "1:50");
        assert_eq!(scale_label(0.5), "2:1");
        assert_eq!(scale_label(1.0), "1:1");
        assert_eq!(
            fit_scale(DVec2::new(120.0, 80.0), DVec2::new(380.0, 230.0)),
            0.5
        );
        assert_eq!(
            fit_scale(DVec2::new(12_000.0, 8000.0), DVec2::new(380.0, 230.0)),
            50.0
        );
    }
}

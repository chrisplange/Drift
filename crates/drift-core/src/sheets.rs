//! Sheets: a paper size and a drawing scale, a view of the sketch, a title block and a
//! revision table.

use std::collections::BTreeSet;

use glam::DVec2;

use crate::{Bounds, LayerId, Rgb};

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
        const IN: f64 = crate::units::MM_PER_INCH;
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
    /// No title block: only the border and centring marks (what "none" meant before blank
    /// paper, so files keep it).
    Border,
    /// Blank paper: nothing drawn.
    None,
}

impl TitleBlock {
    pub const ALL: [TitleBlock; 5] = [
        TitleBlock::Modern,
        TitleBlock::Iso,
        TitleBlock::Ansi,
        TitleBlock::Border,
        TitleBlock::None,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TitleBlock::Modern => "Modern",
            TitleBlock::Iso => "ISO 7200",
            TitleBlock::Ansi => "ASME Y14.1",
            TitleBlock::Border => "Border only",
            TitleBlock::None => "None",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TitleBlock::Modern => "modern",
            TitleBlock::Iso => "iso",
            TitleBlock::Ansi => "ansi",
            TitleBlock::Border => "none",
            TitleBlock::None => "blank",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// The colours a sheet is printed and exported in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SheetColours {
    /// Layer and item colours, as drawn.
    #[default]
    Colour,
    /// Every colour as its luminance grey.
    Greyscale,
    /// Monochrome, as a CAD monochrome plot style: lines, text and hatch lines black, solid
    /// fills their luminance grey (so a dark fill doesn't bury the lines on it). White stays
    /// white: it is the paper's colour (lettering knocked out of a dark box).
    BlackWhite,
}

impl SheetColours {
    pub const ALL: [SheetColours; 3] = [
        SheetColours::Colour,
        SheetColours::Greyscale,
        SheetColours::BlackWhite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SheetColours::Colour => "Colour",
            SheetColours::Greyscale => "Greyscale",
            SheetColours::BlackWhite => "Black & white",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SheetColours::Colour => "colour",
            SheetColours::Greyscale => "greyscale",
            SheetColours::BlackWhite => "mono",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == s)
    }

    /// How `c` prints: as a stroke (lines, text, hatch lines) or as an area (`fill`).
    pub fn map(self, c: Rgb, fill: bool) -> Rgb {
        match self {
            SheetColours::Colour => c,
            SheetColours::BlackWhite if !fill && c != Rgb([255, 255, 255]) => Rgb([0, 0, 0]),
            SheetColours::Greyscale | SheetColours::BlackWhite => {
                let v = luminance(c);
                Rgb([v, v, v])
            }
        }
    }
}

/// The grey a colour prints as (Rec. 601 luma, as print drivers use).
pub fn luminance(c: Rgb) -> u8 {
    let [r, g, b] = c.0.map(f64::from);
    (0.299 * r + 0.587 * g + 0.114 * b)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// One row of a revision table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Revision {
    pub rev: String,
    pub date: String,
    pub description: String,
    pub by: String,
}

/// A sheet's own title block fields. Scale, sheet number and revision are filled in
/// automatically; a title or company left empty is the project's (see [`Project`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleFields {
    pub title: String,
    pub number: String,
    pub author: String,
    pub checked: String,
    pub date: String,
    pub company: String,
    pub material: String,
    /// What the sheet is issued for (`For permit`, `For construction`).
    pub status: String,
}

/// Which fields the title blocks carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FieldSet {
    /// Parts and assemblies: material.
    #[default]
    Mechanical,
    /// Buildings: client, address, project number and status.
    Architectural,
}

impl FieldSet {
    pub const ALL: [FieldSet; 2] = [FieldSet::Mechanical, FieldSet::Architectural];

    pub fn label(self) -> &'static str {
        match self {
            FieldSet::Mechanical => "Mechanical",
            FieldSet::Architectural => "Architectural",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            FieldSet::Mechanical => "mechanical",
            FieldSet::Architectural => "architectural",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == s)
    }
}

/// What every sheet of the drawing shares: the project it is for (Revit's Project
/// Information). A sheet's own title or company, where set, wins over the project's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    /// The project's name: the title on every sheet.
    pub name: String,
    pub number: String,
    pub client: String,
    pub address: String,
    pub company: String,
    pub fields: FieldSet,
}

/// Common sheet statuses, offered when setting one.
pub const STATUSES: [&str; 7] = [
    "Preliminary",
    "For review",
    "For approval",
    "For permit",
    "For tender",
    "For construction",
    "As built",
];

/// Where a view sits on the paper: its middle and its size, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewFrame {
    pub at: DVec2,
    pub size: DVec2,
}

impl ViewFrame {
    pub fn bounds(&self) -> Bounds {
        Bounds {
            min: self.at - self.size * 0.5,
            max: self.at + self.size * 0.5,
        }
    }
}

/// Stable identifier of a view (`w1`: `v` is taken by variables).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(pub u32);

impl std::fmt::Display for ViewId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "w{}", self.0)
    }
}

/// A view of the model: a named crop rectangle in model space, at a scale. It is drawn on
/// the canvas and placed on sheets (several, if need be); reframing it changes every sheet
/// it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// Its title (`Plan`, `Detail A`), written under it on a sheet; may be empty.
    pub name: String,
    /// The drawing scale, 1 : `scale`.
    pub scale: f64,
    /// The world point at the middle of the crop.
    pub center: DVec2,
    /// The crop's width and height in world mm.
    pub size: DVec2,
    /// Show dimensions (ordinates included) in this view.
    pub dimensions: bool,
    /// Show notes and callouts in this view.
    pub notes: bool,
    /// Show construction geometry in this view on sheets: thin, light dashed guides (off by
    /// default; points and an import's fill edges stay out).
    pub construction: bool,
    /// Show the crop and its name on the canvas (Revit's "Crop Region Visible"). Crops never
    /// print.
    pub crop_visible: bool,
    /// Layers left out of this view on sheets and in exports (Revit's visibility/graphics),
    /// whatever their visibility in the model.
    pub hidden_layers: BTreeSet<LayerId>,
}

impl View {
    pub fn new(name: impl Into<String>, crop: Bounds, scale: f64) -> Self {
        Self {
            name: name.into(),
            scale,
            center: crop.center(),
            size: crop.size(),
            dimensions: true,
            notes: true,
            construction: false,
            crop_visible: true,
            hidden_layers: BTreeSet::new(),
        }
    }

    /// The crop in world mm.
    pub fn crop(&self) -> Bounds {
        Bounds {
            min: self.center - self.size * 0.5,
            max: self.center + self.size * 0.5,
        }
    }

    /// Its name, or `View` if it has none.
    pub fn title(&self) -> &str {
        match self.name.trim() {
            "" => "View",
            n => n,
        }
    }

    /// Its size on paper (mm): the crop at the scale.
    pub fn paper_size(&self) -> DVec2 {
        self.size / self.scale
    }
}

/// A view placed on a sheet (a viewport): which view, where its middle is on the paper, and
/// how it is labelled there. Its size on the paper is the view's crop at the view's scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub view: ViewId,
    /// The middle of its frame on the paper (mm).
    pub at: DVec2,
    /// Write the title and scale under the view.
    pub title: bool,
    /// Draw the frame's outline.
    pub border: bool,
}

impl Viewport {
    pub fn new(view: ViewId, at: DVec2) -> Self {
        Self {
            view,
            at,
            title: false,
            border: false,
        }
    }
}

/// A viewport with its view: everything needed to draw it, move it and crop it on a sheet.
/// Made by [`Document::placed_views`](crate::Document::placed_views), and written back with
/// [`Document::set_placed_view`](crate::Document::set_placed_view).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedView {
    pub id: ViewId,
    pub name: String,
    pub scale: f64,
    /// The world point at the middle of the frame.
    pub center: DVec2,
    /// Where it sits on the paper.
    pub frame: ViewFrame,
    pub dimensions: bool,
    pub notes: bool,
    pub construction: bool,
    /// Layers this view leaves out.
    pub hidden_layers: BTreeSet<LayerId>,
    pub title: bool,
    pub border: bool,
}

impl PlacedView {
    pub fn new(id: ViewId, view: &View, port: &Viewport) -> Self {
        Self {
            id,
            name: view.name.clone(),
            scale: view.scale,
            center: view.center,
            frame: ViewFrame {
                at: port.at,
                size: view.paper_size(),
            },
            dimensions: view.dimensions,
            notes: view.notes,
            construction: view.construction,
            hidden_layers: view.hidden_layers.clone(),
            title: port.title,
            border: port.border,
        }
    }

    /// `view` and `port` changed to match this.
    pub fn apply(&self, view: &mut View, port: &mut Viewport) {
        view.name.clone_from(&self.name);
        view.scale = self.scale;
        view.center = self.center;
        view.size = self.frame.size * self.scale;
        view.dimensions = self.dimensions;
        view.notes = self.notes;
        view.construction = self.construction;
        view.hidden_layers.clone_from(&self.hidden_layers);
        port.at = self.frame.at;
        port.title = self.title;
        port.border = self.border;
    }

    /// Its name, or `View` if it has none.
    pub fn title(&self) -> &str {
        match self.name.trim() {
            "" => "View",
            n => n,
        }
    }
}

/// The scale for a title block: the views' scale if they share one, else `As shown` (`-`
/// with none), written for a drawing in feet and inches if `imperial`.
pub fn scale_text(scales: impl IntoIterator<Item = f64>, imperial: bool) -> String {
    let mut scales = scales.into_iter();
    match scales.next() {
        None => "-".into(),
        Some(first) if scales.all(|s| s == first) => scale_label(first, imperial),
        Some(_) => "As shown".into(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub paper: PaperSize,
    pub portrait: bool,
    /// The views placed on the sheet, in drawing order.
    pub viewports: Vec<Viewport>,
    pub title_block: TitleBlock,
    pub fields: TitleFields,
    pub revisions: Vec<Revision>,
    /// The colours it prints and exports in (and shows in the sheet preview).
    pub colours: SheetColours,
}

impl Sheet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            paper: PaperSize::A3,
            portrait: false,
            viewports: Vec::new(),
            title_block: TitleBlock::Modern,
            fields: TitleFields::default(),
            revisions: Vec::new(),
            colours: SheetColours::Colour,
        }
    }

    /// Its title block fields with the project's filled in where its own are empty.
    pub fn fields_with(&self, project: &Project) -> TitleFields {
        let mut f = self.fields.clone();
        if f.title.trim().is_empty() {
            f.title.clone_from(&project.name);
        }
        if f.company.trim().is_empty() {
            f.company.clone_from(&project.company);
        }
        f
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

/// Architectural scales (inches on paper to a foot, and full size and larger as inches to an
/// inch), as 1 : n and as they are written, the largest drawing first.
pub const ARCHITECTURAL_SCALES: [(f64, &str); 15] = [
    (0.5, "2\" = 1\""),
    (1.0, "1\" = 1\""),
    (2.0, "6\" = 1'-0\""),
    (4.0, "3\" = 1'-0\""),
    (8.0, "1-1/2\" = 1'-0\""),
    (12.0, "1\" = 1'-0\""),
    (16.0, "3/4\" = 1'-0\""),
    (24.0, "1/2\" = 1'-0\""),
    (32.0, "3/8\" = 1'-0\""),
    (48.0, "1/4\" = 1'-0\""),
    (64.0, "3/16\" = 1'-0\""),
    (96.0, "1/8\" = 1'-0\""),
    (128.0, "3/32\" = 1'-0\""),
    (192.0, "1/16\" = 1'-0\""),
    (384.0, "1/32\" = 1'-0\""),
];

/// Engineering (civil) scales: an inch on paper to so many feet.
pub const ENGINEERING_SCALES: [(f64, &str); 8] = [
    (120.0, "1\" = 10'"),
    (240.0, "1\" = 20'"),
    (360.0, "1\" = 30'"),
    (480.0, "1\" = 40'"),
    (600.0, "1\" = 50'"),
    (720.0, "1\" = 60'"),
    (1200.0, "1\" = 100'"),
    (2400.0, "1\" = 200'"),
];

/// The smallest and largest n a scale (1 : n) may have.
pub const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.001..=100_000.0;

/// One group of the scale list: its name and its scales (1 : n), the largest drawing first.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleGroup {
    pub name: &'static str,
    pub scales: Vec<f64>,
}

/// The one scale list, for the drawing scale, views and underlays: the groups for the
/// drawing's units first (architectural and engineering for feet and inches, else metric).
pub fn scale_list(imperial: bool) -> Vec<ScaleGroup> {
    let metric = ScaleGroup {
        name: "Metric",
        scales: PREFERRED_SCALES.to_vec(),
    };
    let arch = ScaleGroup {
        name: "Architectural",
        scales: ARCHITECTURAL_SCALES.iter().map(|(n, _)| *n).collect(),
    };
    let eng = ScaleGroup {
        name: "Engineering",
        scales: ENGINEERING_SCALES.iter().map(|(n, _)| *n).collect(),
    };
    if imperial {
        vec![arch, eng, metric]
    } else {
        vec![metric, arch, eng]
    }
}

/// The standard scales, the largest drawing first: metric, or (for imperial drawings) the
/// architectural and engineering scales.
pub fn standard_scales(imperial: bool) -> Vec<f64> {
    if imperial {
        imperial_scales().collect()
    } else {
        PREFERRED_SCALES.to_vec()
    }
}

fn imperial_scales() -> impl Iterator<Item = f64> {
    ARCHITECTURAL_SCALES
        .iter()
        .map(|(n, _)| *n)
        .chain(ENGINEERING_SCALES.iter().map(|(n, _)| *n))
}

/// The largest preferred scale (smallest `n`) at which `extent` (world mm) fits in `room`
/// (paper mm): from the architectural and engineering scales for a drawing in feet and
/// inches, else the metric ones.
pub fn fit_scale(extent: DVec2, room: DVec2, imperial: bool) -> f64 {
    let fits = |n: &f64| extent.x / n <= room.x && extent.y / n <= room.y;
    if imperial {
        imperial_scales().find(fits).unwrap_or(2400.0)
    } else {
        PREFERRED_SCALES
            .into_iter()
            .find(fits)
            .unwrap_or(*PREFERRED_SCALES.last().expect("scales"))
    }
}

/// The scale for a new view of `extent` (world mm) in `room` (paper mm): the drawing scale
/// `preferred` when the drawing fits at it, so the view prints what the canvas shows, else
/// the largest standard scale that fits.
pub fn new_view_scale(extent: DVec2, room: DVec2, imperial: bool, preferred: f64) -> f64 {
    let fits = preferred.is_finite()
        && SCALE_RANGE.contains(&preferred)
        && extent.x / preferred <= room.x
        && extent.y / preferred <= room.y;
    if fits {
        preferred
    } else {
        fit_scale(extent, room, imperial)
    }
}

/// A scale as it is written on a drawing in its units: `1:50`, `2:1`, `1/4" = 1'-0"`,
/// `1" = 20'`, `1" = 1"`. A drawing in feet and inches writes full, half and double size
/// the architectural way, and so any scale that is a whole number of sixteenths of an inch
/// to the foot, whole inches to the inch, or an inch to a whole ten feet or more.
pub fn scale_label(n: f64, imperial: bool) -> String {
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(1.0);
    let listed = ENGINEERING_SCALES
        .iter()
        .chain(
            ARCHITECTURAL_SCALES
                .iter()
                .filter(|(m, _)| imperial || *m > 2.0),
        )
        .find(|(m, _)| near(*m, n));
    if let Some((_, label)) = listed {
        return (*label).to_owned();
    }
    let trim = |v: f64| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    if imperial && n.is_finite() && n > 0.0 {
        // Inches on paper to an inch, when that's a whole number.
        let per_inch = 1.0 / n;
        if per_inch >= 1.0 && near(per_inch, per_inch.round()) {
            return format!("{}\" = 1\"", per_inch.round());
        }
        // Inches on paper to a foot, in sixteenths.
        let per_foot = 12.0 / n;
        let sixteenths = per_foot * 16.0;
        if per_foot < 12.0 && sixteenths >= 1.0 && near(sixteenths, sixteenths.round()) {
            return format!("{}\" = 1'-0\"", inch_fraction(sixteenths.round() as u64));
        }
        // An inch to so many feet.
        let feet = n / 12.0;
        if feet >= 10.0 && near(feet, feet.round()) {
            return format!("1\" = {}'", feet.round());
        }
    }
    if n < 1.0 {
        format!("{}:1", trim(1.0 / n))
    } else {
        format!("1:{}", trim(n))
    }
}

/// `sixteenths` of an inch written as an architect would: `3/8`, `1-1/2`, `5`.
fn inch_fraction(sixteenths: u64) -> String {
    let (whole, mut num) = (sixteenths / 16, sixteenths % 16);
    let mut den = 16;
    while num > 0 && num % 2 == 0 {
        num /= 2;
        den /= 2;
    }
    match (whole, num) {
        (w, 0) => w.to_string(),
        (0, n) => format!("{n}/{den}"),
        (w, n) => format!("{w}-{n}/{den}"),
    }
}

/// Why a typed scale was refused, in words for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaleError(pub String);

impl std::fmt::Display for ScaleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A typed scale, as n in 1 : n: `1:75`, `2:1`, `75` (1:75), or paper `=` model lengths
/// (`3/8" = 1'-0"`, `1/2"=1'`, `1/4 = 1'`, `1" = 20'`, `2" = 1"`, `10 mm = 1 m`). A bare
/// paper length reads in inches when the model length is in feet or inches, else in the
/// model length's unit.
pub fn parse_scale(text: &str) -> Result<f64, ScaleError> {
    use crate::units::{Dim, LengthFormat};
    let text = text.trim();
    let bad = || {
        ScaleError(format!(
            "Not a scale: {text}. Try 1:50, 2:1 or 1/4\" = 1'-0\""
        ))
    };
    if text.is_empty() {
        return Err(ScaleError(
            "Type a scale, such as 1:50 or 1/4\" = 1'-0\"".into(),
        ));
    }
    let number = |s: &str| -> Result<f64, ScaleError> {
        let q = crate::expr::parse(s.trim())
            .and_then(|e| e.eval(&|_| None, LengthFormat::Millimetre))
            .map_err(|_| bad())?;
        if q.dim.is_none() {
            Ok(q.value)
        } else {
            Err(bad())
        }
    };
    let n = if let Some((a, b)) = text.split_once(':') {
        number(b)? / number(a)?
    } else if let Some((paper, model)) = text.split_once('=') {
        let unit = |s: &str| {
            let s = s.trim().to_ascii_lowercase();
            if s.contains('\'') || s.contains('"') || s.ends_with("ft") || s.ends_with("in") {
                Some(LengthFormat::Inch)
            } else if s.ends_with("mm") {
                Some(LengthFormat::Millimetre)
            } else if s.ends_with("cm") {
                Some(LengthFormat::Centimetre)
            } else if s.ends_with('m') {
                Some(LengthFormat::Metre)
            } else {
                None
            }
        };
        // A bare number reads in the other side's unit (inches against feet and inches).
        let default = unit(model).or_else(|| unit(paper)).unwrap_or_default();
        let length = |s: &str| -> Result<f64, ScaleError> {
            let q = crate::expr::parse(s.trim())
                .and_then(|e| e.eval(&|_| None, default))
                .map_err(|_| bad())?;
            if q.dim.is_none() {
                Ok(q.value * default.mm_per_unit())
            } else if q.dim == Dim::LENGTH {
                Ok(q.value)
            } else {
                Err(bad())
            }
        };
        length(model)? / length(paper)?
    } else {
        number(text)?
    };
    if !n.is_finite() || n <= 0.0 {
        return Err(ScaleError(format!(
            "A scale has to be more than zero: {text}"
        )));
    }
    if !SCALE_RANGE.contains(&n) {
        return Err(ScaleError(format!(
            "Out of range: {text} (from 1000:1 to 1:100000)"
        )));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_modes() {
        let red = Rgb([255, 0, 0]);
        let navy = Rgb([0, 0, 128]);
        let white = Rgb([255, 255, 255]);
        assert_eq!(luminance(red), 76);
        assert_eq!(luminance(white), 255);
        assert_eq!(luminance(Rgb([0, 0, 0])), 0);
        // Colour leaves everything alone.
        assert_eq!(SheetColours::Colour.map(red, false), red);
        assert_eq!(SheetColours::Colour.map(navy, true), navy);
        // Greyscale: lines and fills alike, by luminance.
        assert_eq!(SheetColours::Greyscale.map(red, false), Rgb([76, 76, 76]));
        assert_eq!(SheetColours::Greyscale.map(navy, true), Rgb([15, 15, 15]));
        // Fills keep their contrast: a light fill stays lighter than a dark one.
        let pale = Rgb([0xd0, 0xe0, 0xff]);
        assert!(SheetColours::Greyscale.map(pale, true).0[0] > 200);
        // Black & white: every stroke black, fills grey; white (paper) stays white, so
        // lettering knocked out of a dark box still reads.
        for c in [red, navy, Rgb([255, 255, 0])] {
            assert_eq!(SheetColours::BlackWhite.map(c, false), Rgb([0, 0, 0]));
        }
        assert_eq!(SheetColours::BlackWhite.map(white, false), white);
        assert_eq!(SheetColours::BlackWhite.map(red, true), Rgb([76, 76, 76]));
        assert_eq!(SheetColours::BlackWhite.map(white, true), white);
        for c in SheetColours::ALL {
            assert_eq!(SheetColours::parse(c.key()), Some(c));
        }
        assert_eq!(SheetColours::parse("sepia"), None);
        assert_eq!(Sheet::new("A").colours, SheetColours::Colour);
    }

    #[test]
    fn sizes_scales_and_labels() {
        let mut s = Sheet::new("Sheet 1");
        assert_eq!(s.size(), DVec2::new(420.0, 297.0));
        s.portrait = true;
        assert_eq!(s.size(), DVec2::new(297.0, 420.0));
        assert!((PaperSize::AnsiB.sides().0 - 431.8).abs() < 1e-9);
        assert_eq!(PaperSize::parse("ANSI-D"), Some(PaperSize::AnsiD));
        assert_eq!(scale_label(50.0, false), "1:50");
        assert_eq!(scale_label(0.5, false), "2:1");
        assert_eq!(scale_label(1.0, false), "1:1");
        assert_eq!(scale_label(2.0, false), "1:2");
        // Architectural and engineering scales are written their own way.
        assert_eq!(scale_label(48.0, false), "1/4\" = 1'-0\"");
        assert_eq!(scale_label(96.0, true), "1/8\" = 1'-0\"");
        assert_eq!(scale_label(240.0, false), "1\" = 20'");
        // In feet and inches, full, half and double size too.
        assert_eq!(scale_label(1.0, true), "1\" = 1\"");
        assert_eq!(scale_label(2.0, true), "6\" = 1'-0\"");
        assert_eq!(scale_label(0.5, true), "2\" = 1\"");
        // The foot-based scales are none of the metric ones.
        for (n, _) in ARCHITECTURAL_SCALES.iter().filter(|(n, _)| *n > 2.0) {
            assert!(!PREFERRED_SCALES.contains(n), "{n}");
        }
        for (n, _) in &ENGINEERING_SCALES {
            assert!(!PREFERRED_SCALES.contains(n), "{n}");
        }
        // A 40 ft × 30 ft plan on a 24 × 36 sheet's drawing area fits at 1/4" = 1'-0" in
        // feet and inches, at 1:50 in millimetres.
        let plan = DVec2::new(40.0 * 304.8, 30.0 * 304.8);
        let room = DVec2::new(800.0, 560.0);
        assert_eq!(fit_scale(plan, room, true), 24.0);
        assert_eq!(fit_scale(plan * 2.0, room, true), 48.0);
        assert_eq!(fit_scale(plan * 2.0, room, false), 50.0);
        assert_eq!(
            fit_scale(DVec2::new(120.0, 80.0), DVec2::new(380.0, 230.0), false),
            0.5
        );
        assert_eq!(
            fit_scale(
                DVec2::new(12_000.0, 8000.0),
                DVec2::new(380.0, 230.0),
                false
            ),
            50.0
        );
    }

    #[test]
    fn typed_scales() {
        let ok = |t: &str, n: f64| {
            let got = parse_scale(t).unwrap_or_else(|e| panic!("{t}: {e}"));
            assert!((got - n).abs() < 1e-9 * n.max(1.0), "{t}: {got} not {n}");
        };
        ok("1:75", 75.0);
        ok(" 1 : 75 ", 75.0);
        ok("2:1", 0.5);
        ok("1:2.5", 2.5);
        ok("75", 75.0);
        ok("3/8\" = 1'-0\"", 32.0);
        ok("1/2\"=1'", 24.0);
        ok("1/4 = 1'", 48.0);
        ok("1-1/2\" = 1'", 8.0);
        ok("5\" = 1'-0\"", 2.4);
        ok("1\" = 20'", 240.0);
        ok("1\" = 1\"", 1.0);
        ok("2\" = 1\"", 0.5);
        ok("10 mm = 1 m", 100.0);
        ok("1 = 50", 50.0);
        for bad in [
            "", "0:1", "1:0", "-1:50", "1:-50", "abc", "1:x", "0", "1 = 0'", "1:1e9", "1mm:2",
        ] {
            assert!(parse_scale(bad).is_err(), "{bad}");
        }
        // Every listed scale reads back from its label, in either units.
        for imperial in [false, true] {
            for g in scale_list(imperial) {
                for n in g.scales {
                    let label = scale_label(n, imperial);
                    ok(&label, n);
                }
            }
        }
        // Custom scales are written canonically.
        assert_eq!(
            scale_label(parse_scale("1/2\"=1'").unwrap(), true),
            "1/2\" = 1'-0\""
        );
        assert_eq!(scale_label(2.4, true), "5\" = 1'-0\"");
        assert_eq!(scale_label(37.0, true), "1:37");
        assert_eq!(scale_label(37.0, false), "1:37");
        assert_eq!(scale_label(0.25, true), "4\" = 1\"");
        assert_eq!(scale_label(1800.0, true), "1\" = 150'");
    }

    #[test]
    fn new_views_prefer_the_drawing_scale() {
        let room = DVec2::new(380.0, 230.0);
        // A 2 m × 1 m part fits at 1:20: kept, though 1:10 fits too.
        assert_eq!(
            new_view_scale(DVec2::new(2000.0, 1000.0), room, false, 20.0),
            20.0
        );
        // It doesn't fit at 1:5: the largest standard scale that does.
        assert_eq!(
            new_view_scale(DVec2::new(2000.0, 1000.0), room, false, 5.0),
            10.0
        );
        assert_eq!(
            new_view_scale(DVec2::new(2000.0, 1000.0), room, false, f64::NAN),
            10.0
        );
    }
}

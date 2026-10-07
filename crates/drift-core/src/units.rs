//! Quantities with units, and how lengths and angles are displayed.
//!
//! Internally lengths are millimetres and angles degrees (so typed angles stay exact).
//! The display format is a document setting; it never changes stored values.

/// Millimetres per inch (exact by definition).
pub const MM_PER_INCH: f64 = 25.4;

/// The physical dimension of a quantity: powers of length and angle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dim {
    pub length: i8,
    pub angle: i8,
}

impl Dim {
    pub const NONE: Dim = Dim {
        length: 0,
        angle: 0,
    };
    pub const LENGTH: Dim = Dim {
        length: 1,
        angle: 0,
    };
    pub const ANGLE: Dim = Dim {
        length: 0,
        angle: 1,
    };

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }

    pub fn name(self) -> String {
        match (self.length, self.angle) {
            (0, 0) => "a number".into(),
            (1, 0) => "a length".into(),
            (2, 0) => "an area".into(),
            (0, 1) => "an angle".into(),
            (l, a) => format!("length^{l}·angle^{a}"),
        }
    }
}

/// A value in base units (mm, degrees) with its dimension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub dim: Dim,
}

impl Quantity {
    pub fn number(value: f64) -> Self {
        Self {
            value,
            dim: Dim::NONE,
        }
    }

    pub fn length(mm: f64) -> Self {
        Self {
            value: mm,
            dim: Dim::LENGTH,
        }
    }

    pub fn angle(degrees: f64) -> Self {
        Self {
            value: degrees,
            dim: Dim::ANGLE,
        }
    }
}

/// What a value represents: lengths (mm), angles (stored in degrees by dimensions) or plain
/// numbers (counts, ratios).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Length,
    Angle,
    Number,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Length, Kind::Angle, Kind::Number];

    pub fn dim(self) -> Dim {
        match self {
            Kind::Length => Dim::LENGTH,
            Kind::Angle => Dim::ANGLE,
            Kind::Number => Dim::NONE,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Length => "Length",
            Kind::Angle => "Angle",
            Kind::Number => "Number",
        }
    }
}

/// A unit of length a document can display in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LengthFormat {
    #[default]
    Millimetre,
    Centimetre,
    Metre,
    /// Decimal inches (engineering).
    Inch,
    /// Decimal feet (engineering).
    Foot,
    /// Feet and inches with fractions (architectural).
    Architectural,
}

impl LengthFormat {
    pub const METRIC: [LengthFormat; 3] = [
        LengthFormat::Millimetre,
        LengthFormat::Centimetre,
        LengthFormat::Metre,
    ];
    pub const IMPERIAL: [LengthFormat; 3] = [
        LengthFormat::Inch,
        LengthFormat::Foot,
        LengthFormat::Architectural,
    ];

    pub fn is_metric(self) -> bool {
        Self::METRIC.contains(&self)
    }

    /// Millimetres per display unit (architectural values are entered in inches).
    pub fn mm_per_unit(self) -> f64 {
        match self {
            LengthFormat::Millimetre => 1.0,
            LengthFormat::Centimetre => 10.0,
            LengthFormat::Metre => 1000.0,
            LengthFormat::Inch | LengthFormat::Architectural => MM_PER_INCH,
            LengthFormat::Foot => 12.0 * MM_PER_INCH,
        }
    }

    /// The unit name used in expressions (bare numbers are read in this unit).
    pub fn symbol(self) -> &'static str {
        match self {
            LengthFormat::Millimetre => "mm",
            LengthFormat::Centimetre => "cm",
            LengthFormat::Metre => "m",
            LengthFormat::Inch | LengthFormat::Architectural => "in",
            LengthFormat::Foot => "ft",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LengthFormat::Millimetre => "Millimetres",
            LengthFormat::Centimetre => "Centimetres",
            LengthFormat::Metre => "Metres",
            LengthFormat::Inch => "Decimal inches",
            LengthFormat::Foot => "Decimal feet",
            LengthFormat::Architectural => "Feet-inches (fractions)",
        }
    }
}

/// How a document displays lengths and angles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    pub length: LengthFormat,
    /// Decimal places for decimal formats (trailing zeros are trimmed on dimensions).
    pub decimals: u8,
    /// Architectural fractions are rounded to 1/`fraction` inch (a power of two, 2–64).
    pub fraction: u16,
    /// Architectural: show `0'-6"` rather than `6"`.
    pub zero_feet: bool,
    /// Decimal places for angles.
    pub angle_decimals: u8,
}

impl Default for Units {
    fn default() -> Self {
        Self {
            length: LengthFormat::Millimetre,
            decimals: 3,
            fraction: 16,
            zero_feet: false,
            angle_decimals: 2,
        }
    }
}

/// Fraction denominators offered for architectural display.
pub const FRACTIONS: [u16; 6] = [2, 4, 8, 16, 32, 64];

impl Units {
    pub fn metric() -> Self {
        Self::default()
    }

    pub fn architectural() -> Self {
        Self {
            length: LengthFormat::Architectural,
            ..Self::default()
        }
    }

    /// A length as a dimension label shows it: decimal formats trim trailing zeros; imperial
    /// formats carry their marks (`"`, `'`).
    pub fn format_length(&self, mm: f64) -> String {
        match self.length {
            LengthFormat::Architectural => self.feet_inches(mm),
            f => {
                let v = trim(&format!(
                    "{:.*}",
                    usize::from(self.decimals),
                    mm / f.mm_per_unit()
                ));
                match f {
                    LengthFormat::Inch => format!("{v}\""),
                    LengthFormat::Foot => format!("{v}'"),
                    _ => v,
                }
            }
        }
    }

    /// A length for readouts (properties, cursor, status bar): fixed decimals with the unit.
    pub fn format_readout(&self, mm: f64) -> String {
        match self.length {
            LengthFormat::Architectural => self.feet_inches(mm),
            f => {
                let d = usize::from(self.decimals.max(1)) + 1;
                let v = format!("{:.*}", d, mm / f.mm_per_unit() + 0.0);
                match f {
                    LengthFormat::Inch => format!("{v}\""),
                    LengthFormat::Foot => format!("{v}'"),
                    _ => format!("{v} {}", f.symbol()),
                }
            }
        }
    }

    /// True if [`format_length`](Self::format_length) shows `mm` without rounding.
    pub fn is_exact(&self, mm: f64) -> bool {
        let shown = match self.length {
            LengthFormat::Architectural => {
                let n = (mm / MM_PER_INCH * f64::from(self.fraction)).round();
                n / f64::from(self.fraction) * MM_PER_INCH
            }
            f => {
                let scale = 10f64.powi(i32::from(self.decimals));
                (mm / f.mm_per_unit() * scale).round() / scale * f.mm_per_unit()
            }
        };
        (shown - mm).abs() <= 1e-9 * mm.abs().max(1.0)
    }

    /// An angle in degrees, trailing zeros trimmed, with a degree sign.
    pub fn format_angle(&self, degrees: f64) -> String {
        format!(
            "{}°",
            trim(&format!("{:.*}", usize::from(self.angle_decimals), degrees))
        )
    }

    /// A value of `kind` in its stored unit (mm, degrees, plain).
    pub fn format(&self, kind: Kind, value: f64) -> String {
        match kind {
            Kind::Length => self.format_length(value),
            Kind::Angle => self.format_angle(value),
            Kind::Number => trim(&format!("{value:.6}")),
        }
    }

    /// The expression text that reproduces a stored value exactly: in the display unit when
    /// that is exact, else in mm (e.g. a metric value in an imperial document).
    pub fn literal(&self, kind: Kind, value: f64) -> String {
        match kind {
            Kind::Length => {
                let unit = self.length;
                let v = value / unit.mm_per_unit();
                if v * unit.mm_per_unit() == value {
                    format!("{} {}", shortest(v), unit.symbol())
                } else {
                    format!("{} mm", shortest(value))
                }
            }
            Kind::Angle => format!("{} deg", shortest(value)),
            Kind::Number => shortest(value),
        }
    }

    fn feet_inches(&self, mm: f64) -> String {
        let den = i64::from(self.fraction.max(1));
        let n = (mm / MM_PER_INCH * den as f64).round() as i64;
        let sign = if n < 0 { "-" } else { "" };
        let n = n.abs();
        let feet = n / (12 * den);
        let rest = n % (12 * den);
        let inches = rest / den;
        let mut num = rest % den;
        let mut d = den;
        let g = gcd(num, d);
        if g > 0 {
            num /= g;
            d /= g;
        }
        let inch_part = match (inches, num) {
            (i, 0) => format!("{i}\""),
            (0, f) if feet == 0 => format!("{f}/{d}\""),
            (i, f) => format!("{i} {f}/{d}\""),
        };
        if feet > 0 || self.zero_feet {
            format!("{sign}{feet}'-{inch_part}")
        } else {
            format!("{sign}{inch_part}")
        }
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Trim trailing zeros (and a trailing point) from a fixed-point number; `-0` becomes `0`.
fn trim(s: &str) -> String {
    let t = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    };
    if t == "-0" { "0".into() } else { t.into() }
}

/// Shortest text that parses back to exactly `v`.
fn shortest(v: f64) -> String {
    let s = format!("{v}");
    if s == "-0" { "0".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_formats() {
        let u = Units::default();
        assert_eq!(u.format_length(12.5), "12.5");
        assert_eq!(u.format_length(10.0), "10");
        assert_eq!(u.format_readout(12.5), "12.5000 mm");
        let inch = Units {
            length: LengthFormat::Inch,
            ..u
        };
        assert_eq!(inch.format_length(12.375 * MM_PER_INCH), "12.375\"");
        let foot = Units {
            length: LengthFormat::Foot,
            ..u
        };
        assert_eq!(foot.format_length(12.375 * MM_PER_INCH), "1.031'");
        assert!(!foot.is_exact(12.375 * MM_PER_INCH));
        assert!(inch.is_exact(12.375 * MM_PER_INCH));
        assert_eq!(u.format_angle(30.0), "30°");
        assert_eq!(u.format_angle(33.69006), "33.69°");
    }

    #[test]
    fn architectural_formats() {
        let u = Units::architectural();
        let inches = |i: f64| i * MM_PER_INCH;
        assert_eq!(u.format_length(inches(12.375)), "1'-0 3/8\"");
        assert_eq!(u.format_length(inches(18.0)), "1'-6\"");
        assert_eq!(u.format_length(inches(6.5)), "6 1/2\"");
        assert_eq!(u.format_length(inches(0.375)), "3/8\"");
        assert_eq!(u.format_length(inches(24.0)), "2'-0\"");
        assert_eq!(u.format_length(inches(-6.5)), "-6 1/2\"");
        // 4/8 reduces; rounding to 1/16.
        assert_eq!(u.format_length(inches(6.5001)), "6 1/2\"");
        let zero = Units {
            zero_feet: true,
            ..u
        };
        assert_eq!(zero.format_length(inches(6.0)), "0'-6\"");
        // A metric value shown in fractions is rounded.
        assert!(!u.is_exact(10.0));
        assert!(u.is_exact(inches(6.5)));
    }

    #[test]
    fn literals_round_trip() {
        let u = Units {
            length: LengthFormat::Inch,
            ..Units::default()
        };
        assert_eq!(u.literal(Kind::Length, 2.0 * MM_PER_INCH), "2 in");
        assert_eq!(Units::default().literal(Kind::Length, 12.5), "12.5 mm");
        assert_eq!(u.literal(Kind::Angle, 30.0), "30 deg");
    }
}

//! Hatch patterns in AutoCAD `.pat` format, and the built-in library.
//!
//! A pattern is a set of line families. Each family line reads
//! `angle, x-origin, y-origin, delta-x, delta-y [, dash, ...]`: lines at `angle` (degrees)
//! through the origin, each next line shifted by `delta-x` along the line and `delta-y`
//! across it (counter-clockwise of the line's direction). Dashes are lengths along the line:
//! positive draws, negative skips, zero is a dot. Sizes are millimetres unless the file says
//! `;%UNITS=INCH`. A `;%TYPE=MODEL` line marks a model pattern (real-world size), as Revit
//! writes them.

use std::sync::OnceLock;

use drift_core::DVec2;

#[derive(Clone, Debug, PartialEq)]
pub struct PatLine {
    /// Degrees counter-clockwise.
    pub angle: f64,
    pub origin: DVec2,
    /// Shift from one line to the next: `x` along the line, `y` across it.
    pub delta: DVec2,
    pub dashes: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub name: String,
    pub description: String,
    /// A model pattern has real-world size; otherwise it keeps its size on paper.
    pub model: bool,
    pub lines: Vec<PatLine>,
}

impl Pattern {
    /// The closest spacing between lines of any family, at scale 1 (the density limit).
    pub fn min_spacing(&self) -> f64 {
        self.lines
            .iter()
            .map(|l| l.delta.y.abs())
            .filter(|d| *d > 0.0)
            .fold(f64::INFINITY, f64::min)
    }
}

/// Parse the patterns in a `.pat` file.
pub fn parse_pat(text: &str) -> Result<Vec<Pattern>, String> {
    let mut patterns: Vec<Pattern> = Vec::new();
    let mut units = 1.0;
    let mut model = false;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix(';') {
            let c = comment.trim().to_ascii_uppercase().replace(' ', "");
            if c.starts_with("%UNITS=INCH") {
                units = 25.4;
            } else if c.starts_with("%UNITS=MM") {
                units = 1.0;
            } else if c.starts_with("%TYPE=MODEL") {
                // After a header (as Revit writes it) it applies to that pattern; before
                // one, to the next.
                match patterns.last_mut().filter(|p| p.lines.is_empty()) {
                    Some(p) => p.model = true,
                    None => model = true,
                }
            } else if c.starts_with("%TYPE=DRAFTING") {
                model = false;
            }
            continue;
        }
        if let Some(header) = line.strip_prefix('*') {
            let (name, description) = match header.split_once(',') {
                Some((n, d)) => (n.trim(), d.trim()),
                None => (header.trim(), ""),
            };
            if name.is_empty() {
                return Err(format!("line {}: a pattern needs a name", n + 1));
            }
            patterns.push(Pattern {
                name: name.to_owned(),
                description: description.to_owned(),
                model,
                lines: Vec::new(),
            });
            model = false;
            continue;
        }
        let Some(pattern) = patterns.last_mut() else {
            return Err(format!(
                "line {}: pattern lines before a *NAME header",
                n + 1
            ));
        };
        // Trailing comments after the numbers.
        let numbers = line.split(';').next().unwrap_or("");
        let values: Result<Vec<f64>, _> = numbers
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::parse::<f64>)
            .collect();
        let values = values.map_err(|e| format!("line {}: {e}", n + 1))?;
        if values.len() < 5 {
            return Err(format!(
                "line {}: expected angle, x, y, delta-x, delta-y",
                n + 1
            ));
        }
        if values.iter().any(|v| !v.is_finite()) {
            return Err(format!("line {}: not a number", n + 1));
        }
        pattern.lines.push(PatLine {
            angle: values[0],
            origin: DVec2::new(values[1], values[2]) * units,
            delta: DVec2::new(values[3], values[4]) * units,
            dashes: values[5..].iter().map(|d| d * units).collect(),
        });
    }
    patterns.retain(|p| !p.lines.is_empty());
    Ok(patterns)
}

/// The built-in patterns: ANSI31–38 and common material patterns. Drafting patterns are
/// sized for paper; BRICK and BLOCK are model patterns at real size.
const BUILTIN: &str = "\
*ANSI31, Iron, brick, stone masonry
45, 0,0, 0,3.175
*ANSI32, Steel
45, 0,0, 0,9.525
45, 4.49013,0, 0,9.525
*ANSI33, Bronze, brass, copper
45, 0,0, 0,6.35
45, 4.49013,0, 0,6.35, 3.175,-1.5875
*ANSI34, Plastic, rubber
45, 0,0, 0,19.05
45, 4.49013,0, 0,19.05
45, 8.98026,0, 0,19.05
45, 13.4704,0, 0,19.05
*ANSI35, Fire brick, refractory
45, 0,0, 0,6.35
45, 4.49013,0, 0,6.35, 7.9375,-1.5875,0,-1.5875
*ANSI36, Marble, slate, glass
45, 0,0, 5.55625,3.175, 7.9375,-1.5875,0,-1.5875
*ANSI37, Lead, zinc, magnesium, insulation
45, 0,0, 0,3.175
135, 0,0, 0,3.175
*ANSI38, Aluminium
45, 0,0, 0,3.175
135, 0,0, 6.35,3.175, 7.9375,-4.7625
*CONCRETE, Concrete
50, 0,0, 4.12,-5.89, 0.76,-7.63
355, 0,0, -2.28,5.5, 0.6,-6.5
100.45, 0.6,-0.04, 4.27,-6.3, 0.63,-8.48
46.18, 0,2, 6.2,-8.9, 0,-12
96.64, 0.88,1.33, 2.0,-5.2, 0,-9
*EARTH, Earth, soil
0, 0,0, 6.35,6.35, 6.35,-6.35
0, 0,2.38125, 6.35,6.35, 6.35,-6.35
0, 0,4.7625, 6.35,6.35, 6.35,-6.35
90, 0.79375,5.55625, 6.35,6.35, 6.35,-6.35
90, 3.175,5.55625, 6.35,6.35, 6.35,-6.35
90, 5.55625,5.55625, 6.35,6.35, 6.35,-6.35
*INSULATION, Batt insulation
63.4349, 0,0, 2.23607,-4.47214, 5.59017,-5.59017
116.5651, 0,0, -2.23607,-4.47214, 5.59017,-5.59017
*GLASS, Glass
45, 0,0, 3.53553,7.07107, 4,-10.142
45, 1.4,0, 3.53553,7.07107, 2.5,-11.642
*WOOD, Wood grain
0, 0,0, 7,1.5, 14,-2,5,-3
0, 3,0.75, 11,1.5, 9,-4,3,-4
*STEEL, Steel section
45, 0,0, 0,9.525
45, 4.49013,0, 0,9.525
*GRID, Square grid
0, 0,0, 0,3.175
90, 0,0, 0,3.175
*DOTS, Dots
0, 0,0, 1.5875,1.5875, 0,-3.175
*SAND, Sand
0, 0,0, 2.1,2.9, 0,-4.6
0, 1.3,1.1, 1.7,3.3, 0,-5.3
0, 0.6,2.2, 2.8,2.5, 0,-3.9
*BRICK, Brick, running bond (225 × 75 courses)
;%TYPE=MODEL
0, 0,0, 0,75
90, 0,0, 75,112.5, 75,-75
*BLOCK, Concrete block (400 × 200 courses)
;%TYPE=MODEL
0, 0,0, 0,200
90, 0,0, 200,200, 200,-200
";

/// The built-in pattern library.
pub fn builtin() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| parse_pat(BUILTIN).expect("built-in patterns parse"))
}

/// A built-in pattern by name (case-insensitive).
pub fn find(name: &str) -> Option<&'static Pattern> {
    builtin().iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Write patterns back out in `.pat` format (millimetres).
pub fn to_pat(patterns: &[Pattern]) -> String {
    let mut out = String::from(";%UNITS=MM\n");
    for p in patterns {
        out.push('*');
        out.push_str(&p.name);
        if !p.description.is_empty() {
            out.push_str(", ");
            out.push_str(&p.description);
        }
        out.push('\n');
        if p.model {
            out.push_str(";%TYPE=MODEL\n");
        }
        for l in &p.lines {
            let mut fields = vec![l.angle, l.origin.x, l.origin.y, l.delta.x, l.delta.y];
            fields.extend(&l.dashes);
            let text: Vec<String> = fields.iter().map(|v| format!("{v}")).collect();
            out.push_str(&text.join(", "));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse() {
        let all = builtin();
        assert!(all.len() >= 18);
        let ansi31 = find("ansi31").unwrap();
        assert_eq!(ansi31.lines.len(), 1);
        assert_eq!(ansi31.lines[0].delta, DVec2::new(0.0, 3.175));
        assert!(find("BRICK").unwrap().model);
        assert!(!find("ANSI37").unwrap().model);
        assert!((find("ANSI32").unwrap().min_spacing() - 9.525).abs() < 1e-12);
    }

    #[test]
    fn parses_inch_patterns_and_comments() {
        let text = ";; a user pattern\n;%UNITS=INCH\n*MINE, Mine ; comment\n0, 0,0, 0,0.125, 0.25,-0.125 ; trailing\n";
        let p = parse_pat(text).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "MINE");
        assert!((p[0].lines[0].delta.y - 3.175).abs() < 1e-12);
        assert_eq!(p[0].lines[0].dashes.len(), 2);
        assert!(parse_pat("0, 0,0, 0,1\n").is_err());
        assert!(parse_pat("*BAD\n0, 0, x, 0, 1\n").is_err());
    }

    #[test]
    fn round_trips_through_pat_text() {
        let again = parse_pat(&to_pat(builtin())).unwrap();
        assert_eq!(again, builtin());
    }
}

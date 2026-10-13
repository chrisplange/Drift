//! Reading the vector drawing of a PDF page, for use as an underlay: the stroked and filled
//! paths (as their outlines), through nested form XObjects. Text and images are skipped.
//! Coordinates are PDF points with the origin at the page's lower-left corner, upright as
//! the page is shown.

use std::collections::HashSet;

use drift_core::{DVec2, RefPrim};
use lopdf::{Dictionary, Document, Object, ObjectId};

/// The curves of one PDF page.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfPage {
    /// 1-based page number.
    pub number: u32,
    /// How many pages the file has.
    pub pages: u32,
    /// The page size in points, as shown (after its rotation).
    pub size: DVec2,
    pub prims: Vec<RefPrim>,
    /// Text runs and images that were skipped.
    pub skipped_text: usize,
    pub skipped_images: usize,
}

/// Beyond this many segments a page is cut short (it'd be too heavy to trace over).
const MAX_SEGMENTS: usize = 1_000_000;
/// Form XObjects nest at most this deep.
const MAX_DEPTH: usize = 12;

/// An affine transform `[a b c d e f]`, as in PDF: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, p: DVec2) -> DVec2 {
        let [a, b, c, d, e, f] = self.0;
        DVec2::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// `self` applied first, then `then`.
    fn then(&self, then: &Matrix) -> Matrix {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = then.0;
        Matrix([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn nums<const N: usize>(ops: &[Object]) -> Option<[f64; N]> {
    if ops.len() < N {
        return None;
    }
    let mut out = [0.0; N];
    for (i, o) in ops[ops.len() - N..].iter().enumerate() {
        out[i] = num(o)?;
    }
    Some(out)
}

/// The number of pages in a PDF file.
pub fn page_count(bytes: &[u8]) -> Result<u32, String> {
    let doc = Document::load_mem(bytes).map_err(|e| format!("not a readable PDF ({e})"))?;
    Ok(doc.get_pages().len() as u32)
}

/// Read the curves of page `number` (1-based).
pub fn read_page(bytes: &[u8], number: u32) -> Result<PdfPage, String> {
    let doc = Document::load_mem(bytes).map_err(|e| format!("not a readable PDF ({e})"))?;
    if doc.is_encrypted() {
        return Err("the PDF is encrypted".into());
    }
    let pages = doc.get_pages();
    let count = pages.len() as u32;
    let &page_id = pages
        .get(&number)
        .ok_or_else(|| format!("there is no page {number} (the file has {count})"))?;
    // The visible area and the page's rotation (both may be inherited from the page tree).
    let inherited = |key: &[u8]| -> Option<&Object> {
        let mut id = page_id;
        let mut seen = HashSet::new();
        loop {
            let dict = doc.get_dictionary(id).ok()?;
            if let Ok(o) = dict.get(key) {
                return doc.dereference(o).ok().map(|(_, o)| o);
            }
            id = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
            if !seen.insert(id) {
                return None;
            }
        }
    };
    let rect = |o: Option<&Object>| -> Option<[f64; 4]> {
        let a = o?.as_array().ok()?;
        let v: Vec<f64> = a
            .iter()
            .filter_map(|o| num(doc.dereference(o).ok()?.1))
            .collect();
        (v.len() == 4).then(|| {
            [
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ]
        })
    };
    let [x0, y0, x1, y1] = rect(inherited(b"CropBox"))
        .or_else(|| rect(inherited(b"MediaBox")))
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let rotate = inherited(b"Rotate")
        .and_then(num)
        .map_or(0, |r| (r as i64).rem_euclid(360));
    let (w, h) = (x1 - x0, y1 - y0);
    // From PDF user space to the upright page with its lower-left corner at the origin.
    let shift = Matrix([1.0, 0.0, 0.0, 1.0, -x0, -y0]);
    let (turn, size) = match rotate {
        90 => (Matrix([0.0, -1.0, 1.0, 0.0, 0.0, w]), DVec2::new(h, w)),
        180 => (Matrix([-1.0, 0.0, 0.0, -1.0, w, h]), DVec2::new(w, h)),
        270 => (Matrix([0.0, 1.0, -1.0, 0.0, h, 0.0]), DVec2::new(h, w)),
        _ => (Matrix::IDENTITY, DVec2::new(w, h)),
    };
    let base = shift.then(&turn);

    let mut reader = Reader {
        doc: &doc,
        prims: Vec::new(),
        segments: 0,
        skipped_text: 0,
        skipped_images: 0,
    };
    let content = doc.get_page_content(page_id);
    let (resources, inherited_ids) = doc
        .get_page_resources(page_id)
        .unwrap_or((None, Vec::new()));
    let mut res: Vec<&Dictionary> = resources.into_iter().collect();
    res.extend(
        inherited_ids
            .iter()
            .filter_map(|id| doc.get_dictionary(*id).ok()),
    );
    reader.run(&content, base, &res, 0)?;
    Ok(PdfPage {
        number,
        pages: count,
        size,
        prims: reader.prims,
        skipped_text: reader.skipped_text,
        skipped_images: reader.skipped_images,
    })
}

struct Reader<'a> {
    doc: &'a Document,
    prims: Vec<RefPrim>,
    segments: usize,
    skipped_text: usize,
    skipped_images: usize,
}

impl<'a> Reader<'a> {
    /// Find a form or image XObject by name in the resource dictionaries.
    fn xobject(&self, resources: &[&'a Dictionary], name: &[u8]) -> Option<(ObjectId, &'a Object)> {
        for res in resources {
            let Ok(xobjects) = res
                .get_deref(b"XObject", self.doc)
                .and_then(Object::as_dict)
            else {
                continue;
            };
            if let Ok(o) = xobjects.get(name) {
                let id = o.as_reference().ok()?;
                return Some((id, self.doc.get_object(id).ok()?));
            }
        }
        None
    }

    fn run(
        &mut self,
        content: &[u8],
        ctm: Matrix,
        resources: &[&'a Dictionary],
        depth: usize,
    ) -> Result<(), String> {
        let content = lopdf::content::Content::decode(content)
            .map_err(|e| format!("the page's drawing couldn't be read ({e})"))?;
        let mut ctm = ctm;
        let mut stack: Vec<Matrix> = Vec::new();
        // The path being built: finished subpaths and the current one, in page space.
        let mut subpaths: Vec<Vec<DVec2>> = Vec::new();
        let mut current: Vec<DVec2> = Vec::new();
        let mut start = DVec2::ZERO;
        for op in &content.operations {
            if self.segments >= MAX_SEGMENTS {
                break;
            }
            let o = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(ctm),
                "cm" => {
                    if let Some(m) = nums::<6>(o) {
                        ctm = Matrix(m).then(&ctm);
                    }
                }
                "m" => {
                    if let Some([x, y]) = nums::<2>(o) {
                        if current.len() > 1 {
                            subpaths.push(std::mem::take(&mut current));
                        }
                        let p = ctm.apply(DVec2::new(x, y));
                        current = vec![p];
                        start = p;
                    }
                }
                "l" => {
                    if let Some([x, y]) = nums::<2>(o) {
                        current.push(ctm.apply(DVec2::new(x, y)));
                    }
                }
                "c" | "v" | "y" => {
                    let Some(&from) = current.last() else {
                        continue;
                    };
                    let pts: Option<[DVec2; 3]> = match op.operator.as_str() {
                        "c" => nums::<6>(o).map(|[a, b, c, d, e, f]| {
                            [DVec2::new(a, b), DVec2::new(c, d), DVec2::new(e, f)]
                                .map(|p| ctm.apply(p))
                        }),
                        "v" => nums::<4>(o).map(|[c, d, e, f]| {
                            let [p2, p3] =
                                [DVec2::new(c, d), DVec2::new(e, f)].map(|p| ctm.apply(p));
                            [from, p2, p3]
                        }),
                        _ => nums::<4>(o).map(|[a, b, e, f]| {
                            let [p1, p3] =
                                [DVec2::new(a, b), DVec2::new(e, f)].map(|p| ctm.apply(p));
                            [p1, p3, p3]
                        }),
                    };
                    if let Some([p1, p2, p3]) = pts {
                        flatten_cubic(from, p1, p2, p3, &mut current);
                    }
                }
                "re" => {
                    if let Some([x, y, w, h]) = nums::<4>(o) {
                        if current.len() > 1 {
                            subpaths.push(std::mem::take(&mut current));
                        }
                        let corners = [
                            DVec2::new(x, y),
                            DVec2::new(x + w, y),
                            DVec2::new(x + w, y + h),
                            DVec2::new(x, y + h),
                            DVec2::new(x, y),
                        ];
                        subpaths.push(corners.iter().map(|p| ctm.apply(*p)).collect());
                        current.clear();
                        start = ctm.apply(DVec2::new(x, y));
                    }
                }
                "h" => {
                    if current.len() > 1 {
                        current.push(start);
                        subpaths.push(std::mem::take(&mut current));
                        current = vec![start];
                    }
                }
                "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                    let close = matches!(op.operator.as_str(), "s" | "b" | "b*");
                    // Filled shapes trace as their outlines, which are closed.
                    let fill_close = !matches!(op.operator.as_str(), "S" | "s");
                    if current.len() > 1 {
                        if (close || fill_close) && current.first() != current.last() {
                            current.push(start);
                        }
                        subpaths.push(std::mem::take(&mut current));
                    }
                    for mut path in subpaths.drain(..) {
                        if fill_close && path.first() != path.last() && path.len() > 2 {
                            path.push(path[0]);
                        }
                        self.add(path);
                    }
                    current.clear();
                }
                "n" => {
                    subpaths.clear();
                    current.clear();
                }
                "Tj" | "TJ" | "'" | "\"" => self.skipped_text += 1,
                "BI" | "ID" | "EI" => self.skipped_images += usize::from(op.operator == "BI"),
                "Do" => {
                    let Some(name) = o.first().and_then(|n| n.as_name().ok()) else {
                        continue;
                    };
                    let Some((_, obj)) = self.xobject(resources, name) else {
                        continue;
                    };
                    let Ok(stream) = obj.as_stream() else {
                        continue;
                    };
                    let subtype = stream.dict.get(b"Subtype").and_then(Object::as_name).ok();
                    if subtype == Some(b"Image".as_slice()) {
                        self.skipped_images += 1;
                        continue;
                    }
                    if subtype != Some(b"Form".as_slice()) || depth >= MAX_DEPTH {
                        continue;
                    }
                    let matrix = stream
                        .dict
                        .get(b"Matrix")
                        .and_then(Object::as_array)
                        .ok()
                        .and_then(|a| {
                            let v: Vec<f64> = a.iter().filter_map(num).collect();
                            (v.len() == 6).then(|| Matrix([v[0], v[1], v[2], v[3], v[4], v[5]]))
                        })
                        .unwrap_or(Matrix::IDENTITY);
                    let own = stream
                        .dict
                        .get_deref(b"Resources", self.doc)
                        .and_then(Object::as_dict)
                        .ok();
                    let mut res: Vec<&'a Dictionary> = own.into_iter().collect();
                    res.extend_from_slice(resources);
                    let data = stream
                        .decompressed_content()
                        .unwrap_or_else(|_| stream.content.clone());
                    self.run(&data, matrix.then(&ctm), &res, depth + 1)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Keep a painted subpath: drop repeated points; two points make a line.
    fn add(&mut self, path: Vec<DVec2>) {
        let mut points: Vec<DVec2> = Vec::with_capacity(path.len());
        for p in path {
            if !p.is_finite() {
                return;
            }
            // A ten-thousandth of a point is far below anything drawn; it keeps files small.
            let p = (p * 1e4).round() / 1e4;
            if points.last().is_none_or(|q| q.distance(p) > 1e-6) {
                points.push(p);
            }
        }
        match points.len() {
            0 | 1 => {}
            2 => {
                self.segments += 1;
                self.prims.push(RefPrim::Line(points[0], points[1]));
            }
            n => {
                self.segments += n - 1;
                self.prims.push(RefPrim::Polyline(points));
            }
        }
    }
}

/// Append a cubic Bézier from `p0` (already in `out`) as line segments, finely enough that
/// it stays within about a hundredth of a point of the curve.
fn flatten_cubic(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2, out: &mut Vec<DVec2>) {
    let span = p0.distance(p1) + p1.distance(p2) + p2.distance(p3);
    let n = (span.sqrt() * 2.0).ceil().clamp(2.0, 64.0) as usize;
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        out.push(
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::underlays::MM_PER_POINT;
    use drift_draft::page::{Drawing, Pen, Prim};

    fn page_with_curves() -> Vec<u8> {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.push(
            "0",
            Pen::new(0.35),
            Prim::Line(DVec2::new(10.0, 10.0), DVec2::new(110.0, 10.0)),
        );
        d.push(
            "0",
            Pen::new(0.35),
            Prim::Circle {
                center: DVec2::new(150.0, 100.0),
                radius: 20.0,
            },
        );
        crate::pdf::write(&[d.clone(), d], "Test")
    }

    #[test]
    fn reads_lines_and_curves_at_paper_size() {
        let bytes = page_with_curves();
        assert_eq!(page_count(&bytes).unwrap(), 2);
        let page = read_page(&bytes, 1).unwrap();
        assert_eq!(page.pages, 2);
        // A4 landscape in points.
        assert!(
            (page.size.x * MM_PER_POINT - 297.0).abs() < 0.5,
            "{:?}",
            page.size
        );
        let mm = |p: DVec2| p * MM_PER_POINT;
        // The line, at its place on the paper.
        let line = page.prims.iter().find_map(|p| match p {
            RefPrim::Line(a, b) => Some((mm(*a), mm(*b))),
            RefPrim::Polyline(pts) if pts.len() == 2 => Some((mm(pts[0]), mm(pts[1]))),
            _ => None,
        });
        let (a, b) = line.expect("the line");
        assert!(a.distance(DVec2::new(10.0, 10.0)) < 0.01, "{a:?}");
        assert!(b.distance(DVec2::new(110.0, 10.0)) < 0.01, "{b:?}");
        // The circle, flattened: every point is on it.
        let circle = page
            .prims
            .iter()
            .find_map(|p| match p {
                RefPrim::Polyline(pts) if pts.len() > 8 => Some(pts.clone()),
                _ => None,
            })
            .expect("the circle");
        for p in circle {
            let r = mm(p).distance(DVec2::new(150.0, 100.0));
            assert!((r - 20.0).abs() < 0.05, "{r}");
        }
        assert!(read_page(&bytes, 3).is_err());
        assert!(read_page(b"not a pdf", 1).is_err());
    }
}

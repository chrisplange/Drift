//! Small helpers on polygons (closed runs of points) and index sets, written once for the
//! crates that used to carry their own copies.

use glam::DVec2;

/// Whether `p` is inside `poly` by the even-odd rule: a ray from `p` crosses the outline an
/// odd number of times. A loop that overlaps itself has holes where it winds twice; see
/// [`winding`] for the nonzero rule.
pub fn contains(poly: &[DVec2], p: DVec2) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Winding number of `poly` around `p`: how many times the outline goes round it,
/// counter-clockwise positive. Nonzero means inside, whichever way the loop overlaps itself.
pub fn winding(poly: &[DVec2], p: DVec2) -> i32 {
    let mut w = 0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if a.y <= p.y {
            if b.y > p.y && (b - a).perp_dot(p - a) > 0.0 {
                w += 1;
            }
        } else if b.y <= p.y && (b - a).perp_dot(p - a) < 0.0 {
            w -= 1;
        }
    }
    w
}

/// The signed area of `poly` (the shoelace formula): positive counter-clockwise.
pub fn area(poly: &[DVec2]) -> f64 {
    (0..poly.len())
        .map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()]))
        .sum::<f64>()
        / 2.0
}

/// Disjoint sets over the indices `0..n`, joined by [`join`](Self::join). The root of a set
/// is its lowest index, so results don't depend on the order of the joins.
#[derive(Clone, Debug)]
pub struct UnionFind(Vec<usize>);

impl UnionFind {
    pub fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    /// The set `i` is in, as its lowest index.
    pub fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    /// Put `a` and `b` in one set; false if they already were.
    pub fn join(&mut self, a: usize, b: usize) -> bool {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
        a != b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn even_odd_and_nonzero_agree_on_a_simple_loop_and_differ_where_it_winds_twice() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(contains(&square, v(5.0, 5.0)));
        assert!(!contains(&square, v(15.0, 5.0)));
        assert_eq!(winding(&square, v(5.0, 5.0)), 1);
        assert_eq!(winding(&square, v(15.0, 5.0)), 0);
        // The same square gone round twice: even-odd says outside, nonzero says inside.
        let twice: Vec<DVec2> = square.iter().chain(square.iter()).copied().collect();
        assert!(!contains(&twice, v(5.0, 5.0)));
        assert_eq!(winding(&twice, v(5.0, 5.0)), 2);
    }

    #[test]
    fn area_is_signed() {
        let ccw = [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 3.0), v(0.0, 3.0)];
        let cw: Vec<DVec2> = ccw.iter().rev().copied().collect();
        assert_eq!(area(&ccw), 12.0);
        assert_eq!(area(&cw), -12.0);
        assert_eq!(area(&[]), 0.0);
    }

    #[test]
    fn sets_root_at_their_lowest_index_whatever_the_join_order() {
        let mut a = UnionFind::new(5);
        a.join(4, 2);
        a.join(2, 3);
        let mut b = UnionFind::new(5);
        b.join(3, 2);
        b.join(2, 4);
        for i in [2, 3, 4] {
            assert_eq!(a.find(i), 2);
            assert_eq!(b.find(i), 2);
        }
        assert_eq!(a.find(0), 0);
        assert_eq!(a.find(1), 1);
    }
}

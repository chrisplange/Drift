use glam::DVec2;

/// Axis-aligned bounding box in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: DVec2,
    pub max: DVec2,
}

/// How far from the origin (mm) geometry is called far off: 100,000 km, more than twice round
/// the Earth, so beyond anything drawn on purpose (survey coordinates included). A slipped
/// exponent or a stray object in an imported file puts it there, and then fitting the view
/// shows the drawing as a dot.
pub const FAR_MM: f64 = 1e11;

impl Bounds {
    /// Whether the box reaches further than [`FAR_MM`] from the origin, or isn't finite.
    pub fn is_far_off(&self) -> bool {
        !(self.min.is_finite() && self.max.is_finite())
            || self.min.abs().max(self.max.abs()).max_element() > FAR_MM
    }

    /// An empty box that absorbs anything it is unioned with.
    pub const EMPTY: Self = Self {
        min: DVec2::splat(f64::INFINITY),
        max: DVec2::splat(f64::NEG_INFINITY),
    };

    pub fn from_point(p: DVec2) -> Self {
        Self { min: p, max: p }
    }

    pub fn from_center_half_size(center: DVec2, half: DVec2) -> Self {
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn include(&mut self, p: DVec2) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn center(&self) -> DVec2 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> DVec2 {
        self.max - self.min
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

impl FromIterator<Bounds> for Bounds {
    fn from_iter<I: IntoIterator<Item = Bounds>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::union)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_off_is_beyond_a_thousand_km_or_not_finite() {
        let b = |a: DVec2, c: DVec2| {
            let mut b = Bounds::from_point(a);
            b.include(c);
            b
        };
        assert!(!b(DVec2::ZERO, DVec2::splat(FAR_MM)).is_far_off());
        assert!(b(DVec2::ZERO, DVec2::new(0.0, -2e11)).is_far_off());
        assert!(Bounds::from_point(DVec2::new(f64::NAN, 0.0)).is_far_off());
        assert!(Bounds::from_point(DVec2::splat(f64::INFINITY)).is_far_off());
    }
}

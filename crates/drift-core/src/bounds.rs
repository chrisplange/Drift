use glam::DVec2;

/// Axis-aligned bounding box in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: DVec2,
    pub max: DVec2,
}

impl Bounds {
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

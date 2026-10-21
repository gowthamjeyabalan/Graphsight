//! Section box and section lines: a box (a rectangle in the flat view) that cuts the viewport. Nodes outside it are
//! dimmed or left out. Every side of the box is a section line (a plane in 3D) that can be switched on and off on
//! its own, so one line cuts the graph in half, two make a band and all of them make a box.
//!
//! Only what is drawn and what can be clicked changes: the filter, the table and the analysis still see every node.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Outside {
    Dim,
    Hide,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionSettings {
    pub enabled: bool,
    /// What happens to the nodes outside the section.
    pub outside: Outside,
    /// Draw the box / lines in the viewport.
    pub show_frame: bool,
    /// Keep what is outside instead of what is inside.
    pub invert: bool,
    /// Lower and upper limit per axis (x, y, z), in layout units (data units when node coordinates are in use).
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Which of the six section lines / planes are switched on: `[axis][0 = lower, 1 = upper]`.
    pub side_on: [[bool; 2]; 3],
    /// The extent of the nodes when the box was last fitted: the range of the sliders.
    pub range_lo: [f32; 3],
    pub range_hi: [f32; 3],
    /// The limits have been set (by Fit, From selection or by hand); until then the first switch-on fits them.
    pub placed: bool,
}

impl Default for SectionSettings {
    fn default() -> Self {
        SectionSettings {
            enabled: false,
            outside: Outside::Hide,
            show_frame: true,
            invert: false,
            min: [0.0; 3],
            max: [0.0; 3],
            side_on: [[true; 2]; 3],
            range_lo: [0.0; 3],
            range_hi: [0.0; 3],
            placed: false,
        }
    }
}

impl SectionSettings {
    /// True if the point is kept by the section. `dims` is 2 in the flat view and 3 in 3D.
    #[inline]
    pub fn keeps(&self, p: [f32; 3], dims: usize) -> bool {
        let mut inside = true;
        for k in 0..dims {
            if (self.side_on[k][0] && p[k] < self.min[k]) || (self.side_on[k][1] && p[k] > self.max[k]) {
                inside = false;
                break;
            }
        }
        inside != self.invert
    }

    /// Does the section cut anything at all (at least one line on, for the axes that are in use)?
    pub fn any_line(&self, dims: usize) -> bool {
        (0..dims).any(|k| self.side_on[k][0] || self.side_on[k][1])
    }

    /// Set the limits to enclose `pts` (with a little air), and the slider range to match.
    pub fn fit_to(&mut self, pts: impl Iterator<Item = [f32; 3]>) -> bool {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let mut any = false;
        for p in pts {
            if p.iter().all(|v| v.is_finite()) {
                any = true;
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if !any {
            return false;
        }
        for k in 0..3 {
            let pad = ((hi[k] - lo[k]) * 0.02).max(1e-3);
            self.min[k] = lo[k] - pad;
            self.max[k] = hi[k] + pad;
        }
        self.placed = true;
        true
    }

    /// Fit the slider range (not the limits) to `pts`.
    pub fn set_range(&mut self, pts: impl Iterator<Item = [f32; 3]>) {
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let mut any = false;
        for p in pts {
            if p.iter().all(|v| v.is_finite()) {
                any = true;
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        if any {
            for k in 0..3 {
                let pad = ((hi[k] - lo[k]) * 0.25).max(1.0);
                self.range_lo[k] = lo[k] - pad;
                self.range_hi[k] = hi[k] + pad;
            }
        }
    }

    /// Keep the limits in order (lower never above upper).
    pub fn normalise(&mut self) {
        for k in 0..3 {
            if self.min[k] > self.max[k] {
                std::mem::swap(&mut self.min[k], &mut self.max[k]);
            }
        }
    }
}

/// The eight corners of the box, in the order of `boxes::EDGES` / `boxes::FACES` (bit 0 = x, bit 1 = y, bit 2 = z).
pub fn corner(s: &SectionSettings, k: usize) -> [f32; 3] {
    [0, 1, 2].map(|a| if (k >> a) & 1 == 1 { s.max[a] } else { s.min[a] })
}

/// The twelve edges of the box as pairs of corner indices.
pub const EDGES: [(usize, usize, usize); 12] = [
    // (corner, corner, axis the edge runs along)
    (0, 1, 0),
    (2, 3, 0),
    (4, 5, 0),
    (6, 7, 0),
    (0, 2, 1),
    (1, 3, 1),
    (4, 6, 1),
    (5, 7, 1),
    (0, 4, 2),
    (1, 5, 2),
    (2, 6, 2),
    (3, 7, 2),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed() -> SectionSettings {
        SectionSettings { enabled: true, min: [0.0, 0.0, 0.0], max: [10.0, 10.0, 10.0], placed: true, ..Default::default() }
    }

    #[test]
    fn a_box_keeps_what_is_inside() {
        let s = boxed();
        assert!(s.keeps([5.0, 5.0, 99.0], 2), "z is ignored in the flat view");
        assert!(!s.keeps([5.0, 5.0, 99.0], 3));
        assert!(!s.keeps([-1.0, 5.0, 5.0], 2));
        assert!(!s.keeps([5.0, 11.0, 5.0], 2));
    }

    #[test]
    fn single_lines_make_half_planes_and_bands() {
        let mut s = boxed();
        s.side_on = [[false, true], [false; 2], [false; 2]]; // only "x <= 10"
        assert!(s.keeps([-100.0, 500.0, 0.0], 2));
        assert!(!s.keeps([11.0, 0.0, 0.0], 2));
        s.side_on = [[true, true], [false; 2], [false; 2]]; // a band 0 <= x <= 10
        assert!(!s.keeps([-1.0, 0.0, 0.0], 2) && !s.keeps([11.0, 0.0, 0.0], 2) && s.keeps([5.0, 1e6, 0.0], 2));
        s.side_on = [[false; 2]; 3];
        assert!(!s.any_line(3) && s.keeps([1e9, 1e9, 1e9], 3), "no line on: nothing is cut");
    }

    #[test]
    fn invert_keeps_the_outside() {
        let mut s = boxed();
        s.invert = true;
        assert!(!s.keeps([5.0, 5.0, 5.0], 3));
        assert!(s.keeps([50.0, 5.0, 5.0], 3));
    }

    #[test]
    fn fit_encloses_the_points_and_corners_are_ordered() {
        let mut s = SectionSettings::default();
        assert!(s.fit_to([[0.0, 0.0, 0.0], [10.0, 20.0, 30.0]].into_iter()));
        assert!(s.min[0] < 0.0 && s.max[2] > 30.0);
        assert!(!SectionSettings::default().fit_to(std::iter::empty()));
        s.min = [0.0; 3];
        s.max = [1.0, 2.0, 3.0];
        assert_eq!(corner(&s, 0), [0.0, 0.0, 0.0]);
        assert_eq!(corner(&s, 7), [1.0, 2.0, 3.0]);
        assert_eq!(corner(&s, 5), [1.0, 0.0, 3.0]);
        // every edge differs in exactly one axis
        for (a, b, axis) in EDGES {
            let (p, q) = (corner(&s, a), corner(&s, b));
            let diff: Vec<usize> = (0..3).filter(|&k| p[k] != q[k]).collect();
            assert_eq!(diff, vec![axis]);
        }
    }
}

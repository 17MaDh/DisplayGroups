//! Desktop rectangles in physical pixels.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self {
            x: x.into(),
            y: y.into(),
            w: w.into(),
            h: h.into(),
        }
    }

    pub fn right(&self) -> i64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i64 {
        self.y + self.h
    }

    /// True when the rectangles share a positive area.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// True when the rectangles meet along an edge or at a corner without overlapping.
    pub fn touches(&self, other: &Rect) -> bool {
        if self.overlaps(other) {
            return false;
        }
        let vertical_contact = self.y.max(other.y) <= self.bottom().min(other.bottom());
        let horizontal_contact = self.x.max(other.x) <= self.right().min(other.right());
        let side_by_side = self.right() == other.x || other.right() == self.x;
        let stacked = self.bottom() == other.y || other.bottom() == self.y;
        (side_by_side && vertical_contact) || (stacked && horizontal_contact)
    }
}

/// True when every rectangle is reachable from the first one through touching rectangles.
pub fn is_connected(rects: &[Rect]) -> bool {
    if rects.len() <= 1 {
        return true;
    }
    let mut seen = vec![false; rects.len()];
    let mut stack = vec![0usize];
    seen[0] = true;
    while let Some(i) = stack.pop() {
        for j in 0..rects.len() {
            if !seen[j] && rects[i].touches(&rects[j]) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen.into_iter().all(|s| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_desk_layout_touches() {
        // DFI27280 left of the ASUS, vertically offset (from the test machine).
        let dfi = Rect::new(-1920, 457, 1920, 1080);
        let asus = Rect::new(0, 0, 3840, 2160);
        assert!(!dfi.overlaps(&asus));
        assert!(dfi.touches(&asus));
        assert!(is_connected(&[asus, dfi]));
    }

    #[test]
    fn overlap_and_gap_detected() {
        let a = Rect::new(0, 0, 1920, 1080);
        assert!(a.overlaps(&Rect::new(1000, 0, 1920, 1080)));
        let far = Rect::new(2000, 0, 1920, 1080);
        assert!(!a.touches(&far));
        assert!(!is_connected(&[a, far]));
    }

    #[test]
    fn corner_contact_counts_as_touching() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(100, 100, 100, 100);
        assert!(a.touches(&b));
    }
}

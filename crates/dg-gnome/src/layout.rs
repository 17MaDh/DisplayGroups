//! Converting a layout between coordinate spaces.
//!
//! The config stores positions the way Windows does: every display is a rectangle of its
//! physical pixels. Mutter's logical layout (Wayland) sizes each display by its mode
//! divided by its scale, so a 3840×2160 display at 150% is 2560×1440 there. Positions
//! cannot simply be divided by one scale when displays use different scales, so the
//! arrangement is rebuilt from how the displays touch: the primary stays at (0,0), and
//! each neighbour keeps its side and its offset along the shared edge, measured in the
//! units of the display it is attached to.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn right(&self) -> i32 {
        self.x + self.w
    }
    fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Above,
    Below,
}

/// Which side of `parent` the rectangle `child` is attached to, if they touch.
fn attached(parent: &Rect, child: &Rect) -> Option<Side> {
    let overlaps_v = child.y <= parent.bottom() && child.bottom() >= parent.y;
    let overlaps_h = child.x <= parent.right() && child.right() >= parent.x;
    if child.right() == parent.x && overlaps_v {
        Some(Side::Left)
    } else if child.x == parent.right() && overlaps_v {
        Some(Side::Right)
    } else if child.bottom() == parent.y && overlaps_h {
        Some(Side::Above)
    } else if child.y == parent.bottom() && overlaps_h {
        Some(Side::Below)
    } else {
        None
    }
}

/// Places displays in another coordinate space.
///
/// * `from`: the rectangles in the source space.
/// * `sizes`: each display's size in the target space.
/// * `units`: for each display, target units per source unit for offsets measured
///   against it (for physical → logical that is `1 / scale` of that display).
/// * `primary`: the display placed at (0,0).
///
/// Displays that touch nothing are put to the right of the others, top-aligned.
pub fn relayout(from: &[Rect], sizes: &[(i32, i32)], units: &[f64], primary: usize) -> Vec<Rect> {
    let n = from.len();
    let mut placed: Vec<Option<Rect>> = vec![None; n];
    if n == 0 {
        return Vec::new();
    }
    placed[primary] = Some(Rect {
        x: 0,
        y: 0,
        w: sizes[primary].0,
        h: sizes[primary].1,
    });
    let mut queue = vec![primary];
    while let Some(p) = queue.pop() {
        let parent_src = from[p];
        let parent = placed[p].expect("queued only when placed");
        for c in 0..n {
            if placed[c].is_some() {
                continue;
            }
            let Some(side) = attached(&parent_src, &from[c]) else {
                continue;
            };
            let (w, h) = sizes[c];
            let along = |src_offset: i32| (f64::from(src_offset) * units[p]).round() as i32;
            let rect = match side {
                Side::Left | Side::Right => {
                    let y = (parent.y + along(from[c].y - parent_src.y))
                        .clamp(parent.y - h, parent.bottom());
                    let x = if side == Side::Left {
                        parent.x - w
                    } else {
                        parent.right()
                    };
                    Rect { x, y, w, h }
                }
                Side::Above | Side::Below => {
                    let x = (parent.x + along(from[c].x - parent_src.x))
                        .clamp(parent.x - w, parent.right());
                    let y = if side == Side::Above {
                        parent.y - h
                    } else {
                        parent.bottom()
                    };
                    Rect { x, y, w, h }
                }
            };
            placed[c] = Some(rect);
            queue.push(c);
        }
    }
    // Anything left over touched nothing: line it up on the right.
    let mut right = placed.iter().flatten().map(Rect::right).max().unwrap_or(0);
    let mut rest: Vec<usize> = (0..n).filter(|&i| placed[i].is_none()).collect();
    rest.sort_by_key(|&i| (from[i].x, from[i].y));
    for i in rest {
        let (w, h) = sizes[i];
        placed[i] = Some(Rect {
            x: right,
            y: 0,
            w,
            h,
        });
        right += w;
    }
    placed.into_iter().map(|r| r.expect("all placed")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn mixed_scales_keep_sides_and_offsets() {
        // Physical: 4K at 150% as primary, a 1080p display to its left, 474 px lower.
        let physical = [r(0, 0, 3840, 2160), r(-1920, 474, 1920, 1080)];
        let logical = relayout(
            &physical,
            &[(2560, 1440), (1920, 1080)],
            &[1.0 / 1.5, 1.0],
            0,
        );
        assert_eq!(
            logical,
            vec![r(0, 0, 2560, 1440), r(-1920, 316, 1920, 1080)]
        );
        // And back.
        let again = relayout(&logical, &[(3840, 2160), (1920, 1080)], &[1.5, 1.0], 0);
        assert_eq!(again, physical.to_vec());
    }

    #[test]
    fn primary_need_not_be_at_the_origin_in_the_source() {
        let src = [r(1920, 0, 1920, 1080), r(0, 0, 1920, 1080)];
        let out = relayout(&src, &[(1920, 1080), (1920, 1080)], &[1.0, 1.0], 0);
        assert_eq!(out, vec![r(0, 0, 1920, 1080), r(-1920, 0, 1920, 1080)]);
    }

    #[test]
    fn chains_and_stacks_follow_their_neighbours() {
        let src = [
            r(0, 0, 2000, 1000),
            r(2000, 0, 1000, 1000),
            r(2000, 1000, 1000, 500),
        ];
        let out = relayout(
            &src,
            &[(1000, 500), (500, 500), (500, 250)],
            &[0.5, 0.5, 0.5],
            0,
        );
        assert_eq!(
            out,
            vec![
                r(0, 0, 1000, 500),
                r(1000, 0, 500, 500),
                r(1000, 500, 500, 250)
            ]
        );
    }

    #[test]
    fn detached_displays_are_lined_up_on_the_right() {
        let src = [r(0, 0, 100, 100), r(500, 0, 100, 100)];
        let out = relayout(&src, &[(100, 100), (100, 100)], &[1.0, 1.0], 0);
        assert_eq!(out[1], r(100, 0, 100, 100));
    }
}

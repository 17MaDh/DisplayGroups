//! Refresh-rate selection with the "best at or below" fallback.

use dg_model::RefreshPolicy;

use crate::backend::DeviceMode;

/// Rates within half a hertz count as the same rate (119.88 vs 120).
pub const TOLERANCE_MILLI_HZ: u32 = 500;

/// Picks a whole-hertz rate for a `width`×`height` mode (either orientation).
///
/// * `Exact`: a rate within tolerance of the target.
/// * `BestAtOrBelow`: the highest rate not above the target (plus tolerance).
pub fn pick_refresh_hz(
    modes: &[DeviceMode],
    width: u32,
    height: u32,
    want_milli_hz: u32,
    policy: RefreshPolicy,
) -> Option<u32> {
    let rates = modes
        .iter()
        .filter(|m| {
            (m.width, m.height) == (width, height) || (m.width, m.height) == (height, width)
        })
        .map(|m| m.refresh_hz)
        // 0 and 1 mean "hardware default" in DEVMODE.
        .filter(|&hz| hz > 1);
    match policy {
        RefreshPolicy::Exact => rates
            .filter(|&hz| (hz * 1000).abs_diff(want_milli_hz) <= TOLERANCE_MILLI_HZ)
            .max(),
        RefreshPolicy::BestAtOrBelow => rates
            .filter(|&hz| hz * 1000 <= want_milli_hz.saturating_add(TOLERANCE_MILLI_HZ))
            .max(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(list: &[(u32, u32, u32)]) -> Vec<DeviceMode> {
        list.iter()
            .map(|&(width, height, refresh_hz)| DeviceMode {
                width,
                height,
                refresh_hz,
                x: 0,
                y: 0,
                orientation: 0,
            })
            .collect()
    }

    #[test]
    fn best_at_or_below_falls_back() {
        let tv = modes(&[
            (3840, 2160, 60),
            (3840, 2160, 100),
            (3840, 2160, 119),
            (1920, 1080, 120),
        ]);
        assert_eq!(
            pick_refresh_hz(&tv, 3840, 2160, 120_000, RefreshPolicy::BestAtOrBelow),
            Some(119)
        );
        assert_eq!(
            pick_refresh_hz(&tv, 3840, 2160, 119_880, RefreshPolicy::BestAtOrBelow),
            Some(119)
        );
        assert_eq!(
            pick_refresh_hz(&tv, 3840, 2160, 50_000, RefreshPolicy::BestAtOrBelow),
            None
        );
    }

    #[test]
    fn exact_requires_a_match_within_tolerance() {
        let tv = modes(&[(3840, 2160, 60), (3840, 2160, 120)]);
        assert_eq!(
            pick_refresh_hz(&tv, 3840, 2160, 119_880, RefreshPolicy::Exact),
            Some(120)
        );
        assert_eq!(
            pick_refresh_hz(&tv, 3840, 2160, 100_000, RefreshPolicy::Exact),
            None
        );
    }

    #[test]
    fn portrait_modes_match_and_defaults_are_ignored() {
        let m = modes(&[(1080, 1920, 60), (1920, 1080, 1)]);
        assert_eq!(
            pick_refresh_hz(&m, 1920, 1080, 60_000, RefreshPolicy::BestAtOrBelow),
            Some(60)
        );
        assert_eq!(
            pick_refresh_hz(&m, 1920, 1080, 1_000, RefreshPolicy::BestAtOrBelow),
            None
        );
    }
}

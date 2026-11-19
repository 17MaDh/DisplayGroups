//! Per-monitor scale. Windows exposes it only relative to its "recommended" step,
//! through undocumented `DisplayConfig{Get,Set}DeviceInfo` packet types -3 / -4.

use dg_model::SCALE_STEPS;
use serde::{Deserialize, Serialize};

/// Raw values from the GET packet: offsets from the recommended step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DpiInfo {
    pub min_rel: i32,
    pub cur_rel: i32,
    pub max_rel: i32,
}

impl DpiInfo {
    /// The lowest step is always 100%, so the recommended step sits `-min_rel` above it.
    pub fn recommended_index(&self) -> Option<usize> {
        usize::try_from(-i64::from(self.min_rel))
            .ok()
            .filter(|&i| i < SCALE_STEPS.len())
    }

    pub fn percent_at(&self, rel: i32) -> Option<u32> {
        let index = i64::try_from(self.recommended_index()?).ok()? + i64::from(rel);
        usize::try_from(index)
            .ok()
            .and_then(|i| SCALE_STEPS.get(i).copied())
    }

    pub fn current_percent(&self) -> Option<u32> {
        self.percent_at(self.cur_rel)
    }

    pub fn recommended_percent(&self) -> Option<u32> {
        self.percent_at(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpiPlan {
    Unchanged {
        percent: u32,
    },
    Change {
        from_percent: Option<u32>,
        to_percent: u32,
        relative: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DpiError {
    #[error("{0}% is not a Windows scale step")]
    UnsupportedPercent(u32),
    #[error("{want}% is outside what Windows allows for this display ({min}%–{max}%)")]
    OutOfRange { want: u32, min: u32, max: u32 },
    #[error("Windows returned scale data this build does not understand ({0:?})")]
    Unreadable(DpiInfo),
}

/// Works out the relative value to send for `want` percent, with bounds checks.
pub fn plan(info: DpiInfo, want: u32) -> Result<DpiPlan, DpiError> {
    let rec = info.recommended_index().ok_or(DpiError::Unreadable(info))?;
    if info.min_rel > info.max_rel || info.cur_rel < info.min_rel || info.cur_rel > info.max_rel {
        return Err(DpiError::Unreadable(info));
    }
    let want_index = SCALE_STEPS
        .iter()
        .position(|&s| s == want)
        .ok_or(DpiError::UnsupportedPercent(want))?;
    let relative = i32::try_from(want_index).unwrap_or(i32::MAX) - i32::try_from(rec).unwrap_or(0);
    if relative < info.min_rel || relative > info.max_rel {
        return Err(DpiError::OutOfRange {
            want,
            min: info.percent_at(info.min_rel).unwrap_or(100),
            max: info.percent_at(info.max_rel).unwrap_or(want),
        });
    }
    if relative == info.cur_rel {
        return Ok(DpiPlan::Unchanged { percent: want });
    }
    Ok(DpiPlan::Change {
        from_percent: info.current_percent(),
        to_percent: want,
        relative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Samsung TV on the test machine: recommended 300% (index 7), user wants 175%.
    const TV: DpiInfo = DpiInfo {
        min_rel: -7,
        cur_rel: 0,
        max_rel: 2,
    };

    #[test]
    fn tv_reset_to_recommended_is_corrected_to_175() {
        assert_eq!(TV.recommended_percent(), Some(300));
        assert_eq!(TV.current_percent(), Some(300));
        assert_eq!(
            plan(TV, 175),
            Ok(DpiPlan::Change {
                from_percent: Some(300),
                to_percent: 175,
                relative: -4
            })
        );
    }

    #[test]
    fn already_at_target_is_unchanged() {
        let info = DpiInfo { cur_rel: -4, ..TV };
        assert_eq!(info.current_percent(), Some(175));
        assert_eq!(plan(info, 175), Ok(DpiPlan::Unchanged { percent: 175 }));
    }

    #[test]
    fn bounds_and_garbage_are_rejected() {
        assert_eq!(plan(TV, 130), Err(DpiError::UnsupportedPercent(130)));
        assert!(matches!(
            plan(TV, 450),
            Err(DpiError::OutOfRange { want: 450, .. })
        ));
        let garbage = DpiInfo {
            min_rel: -40,
            cur_rel: 0,
            max_rel: 3,
        };
        assert_eq!(plan(garbage, 100), Err(DpiError::Unreadable(garbage)));
        let inverted = DpiInfo {
            min_rel: -2,
            cur_rel: 5,
            max_rel: 1,
        };
        assert!(matches!(plan(inverted, 100), Err(DpiError::Unreadable(_))));
    }
}

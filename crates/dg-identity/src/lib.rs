//! Stable monitor identity (ARCHITECTURE.md §4): EDID parsing, CCD id decoding,
//! device-path helpers and the tiered matching algorithm. No Win32 code.

pub mod edid;
pub mod matching;
pub mod naming;

pub use edid::{decode_pnp, parse_edid, pnp_from_ccd, product_from_ccd, Edid, EdidError};
pub use matching::{match_monitor, MatchError, MatchTier, Matched, Observed};
pub use naming::{connector_hint, hardware_id, instance_id_from_device_path, monitor_key};

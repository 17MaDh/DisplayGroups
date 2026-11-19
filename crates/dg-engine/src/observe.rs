//! Turning CCD target details into identity data for matching and capture.

use dg_identity::{
    connector_hint, hardware_id, parse_edid, pnp_from_ccd, product_from_ccd, Observed,
};
use dg_model::MonitorIdentity;

use crate::ccd::{output_technology, TargetDetails};

/// Identity data for a connected target. Prefers the EDID; falls back to the ids CCD
/// reports, then to the hardware id in the device path.
pub fn observe(d: &TargetDetails) -> Observed {
    let edid = d.edid.as_deref().and_then(|b| parse_edid(b).ok());
    let (pnp_id, product_code) = match (&edid, d.edid_manufacture_id, d.edid_product_code) {
        (Some(e), _, _) => (e.pnp_id.clone(), e.product_code.clone()),
        (None, Some(m), Some(p)) => (pnp_from_ccd(m), product_from_ccd(p)),
        _ => split_hardware_id(&d.device_path),
    };
    let friendly_name = if d.friendly_name.trim().is_empty() {
        edid.as_ref()
            .and_then(|e| e.name.clone())
            .unwrap_or_default()
    } else {
        d.friendly_name.trim().to_string()
    };
    Observed {
        pnp_id,
        product_code,
        serial: edid.as_ref().and_then(|e| e.serial()),
        friendly_name,
        connector_hint: connector_hint(&d.device_path),
    }
}

pub fn identity_of(d: &TargetDetails) -> MonitorIdentity {
    let o = observe(d);
    MonitorIdentity {
        pnp_id: o.pnp_id,
        product_code: o.product_code,
        serial: o.serial,
        friendly_name: o.friendly_name,
        connector_hint: o.connector_hint,
        adapter_hint: None,
        output_technology: Some(output_technology::name(d.output_technology).to_string()),
        extra: Default::default(),
    }
}

/// A human-friendly default name for a newly captured monitor.
pub fn display_name_of(d: &TargetDetails) -> String {
    let o = observe(d);
    if !o.friendly_name.is_empty() {
        o.friendly_name
    } else {
        format!("{} {}", o.pnp_id, o.product_code)
    }
}

fn split_hardware_id(device_path: &str) -> (String, String) {
    match hardware_id(device_path) {
        Some(hw) if hw.len() > 3 && hw.is_char_boundary(3) => {
            (hw[..3].to_ascii_uppercase(), hw[3..].to_ascii_uppercase())
        }
        Some(hw) => (hw.to_ascii_uppercase(), String::new()),
        None => (String::new(), String::new()),
    }
}

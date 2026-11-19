//! Identity of a Mutter monitor, in the same terms as on Windows, so a config captured
//! on one system recognises the same monitors on the other.

use dg_identity::{parse_edid, Observed};
use dg_model::MonitorIdentity;

use crate::state::Monitor;

/// Prefers the raw EDID (exact product code and serial, as Windows reads them); falls
/// back to Mutter's monitor spec.
pub fn observe(m: &Monitor) -> Observed {
    let edid = m.edid.as_deref().and_then(|b| parse_edid(b).ok());
    let (pnp_id, product_code, serial, name) = match &edid {
        Some(e) => (
            e.pnp_id.clone(),
            e.product_code.clone(),
            e.serial(),
            e.name.clone().unwrap_or_else(|| m.product.clone()),
        ),
        // Mutter writes the product code as "0x721b" when the EDID has no product name,
        // and the numeric serial as "0x16780800" when it has no serial string.
        None => match hex(&m.product) {
            Some(code) => (
                m.vendor.clone(),
                format!("{code:04X}"),
                serial_of(&m.serial),
                String::new(),
            ),
            None => (
                m.vendor.clone(),
                m.product.clone(),
                serial_of(&m.serial),
                m.product.clone(),
            ),
        },
    };
    Observed {
        pnp_id,
        product_code,
        serial,
        friendly_name: name.trim().to_string(),
        connector_hint: Some(m.connector.clone()),
    }
}

pub fn identity_of(m: &Monitor) -> MonitorIdentity {
    let o = observe(m);
    MonitorIdentity {
        pnp_id: o.pnp_id,
        product_code: o.product_code,
        serial: o.serial,
        friendly_name: o.friendly_name,
        connector_hint: o.connector_hint,
        adapter_hint: None,
        output_technology: Some(output_technology(&m.connector).to_string()),
        extra: Default::default(),
    }
}

/// The name a newly seen monitor gets in the config.
pub fn display_name_of(m: &Monitor) -> String {
    let o = observe(m);
    if !o.friendly_name.is_empty() {
        o.friendly_name
    } else if !m.display_name.is_empty() {
        m.display_name.clone()
    } else {
        format!("{} {}", o.pnp_id, o.product_code)
    }
}

/// The serial as Windows reads it: a serial string as is, a numeric serial in decimal.
/// Mutter writes numeric serials as hex ("0x16780800") and 0 when there is none.
fn serial_of(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("unknown") {
        return None;
    }
    match hex(s) {
        // The same placeholders the EDID parser ignores.
        Some(0 | 0x0101_0101 | 0xFFFF_FFFF) => None,
        Some(n) => Some(n.to_string()),
        None => Some(s.to_string()),
    }
}

fn hex(s: &str) -> Option<u32> {
    let digits = s.trim().strip_prefix("0x")?;
    u32::from_str_radix(digits, 16).ok()
}

/// The same names Windows uses for `outputTechnology`.
fn output_technology(connector: &str) -> &'static str {
    let kind = connector.rsplit_once('-').map_or(connector, |(k, _)| k);
    match kind {
        "HDMI" | "HDMI-A" | "HDMI-B" => "hdmi",
        "DP" => "displayport",
        "eDP" => "displayportEmbedded",
        "DVI-I" | "DVI-D" | "DVI-A" => "dvi",
        "VGA" => "vga",
        "LVDS" | "DSI" => "internal",
        _ => "other",
    }
}

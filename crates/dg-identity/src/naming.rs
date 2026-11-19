//! Helpers for monitor device paths and config keys.
//!
//! A CCD monitor device path looks like
//! `\\?\DISPLAY#SAM721B#5&2031f481&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}`.

/// `UIDnnnn` from the device path: identifies the connector instance, which tells
/// identical monitors apart.
pub fn connector_hint(device_path: &str) -> Option<String> {
    let upper = device_path.to_ascii_uppercase();
    let start = upper.find("UID")?;
    let digits: String = upper[start + 3..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("UID{digits}"))
}

/// The PnP hardware id segment, e.g. `SAM721B`.
pub fn hardware_id(device_path: &str) -> Option<String> {
    let mut parts = device_path.trim_start_matches(r"\\?\").split('#');
    let _class = parts.next()?;
    parts.next().filter(|s| !s.is_empty()).map(str::to_string)
}

/// Device instance id under `HKLM\SYSTEM\CurrentControlSet\Enum`, e.g.
/// `DISPLAY\SAM721B\5&2031f481&0&UID4352`. The EDID lives in its `Device Parameters`.
pub fn instance_id_from_device_path(device_path: &str) -> Option<String> {
    let parts: Vec<&str> = device_path.trim_start_matches(r"\\?\").split('#').collect();
    if parts.len() < 3 || parts[..3].iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(parts[..3].join(r"\"))
}

/// Builds a readable, stable config key such as `mon-sam-721b-376965120`.
pub fn monitor_key(
    pnp_id: &str,
    product_code: &str,
    serial: Option<&str>,
    connector: Option<&str>,
) -> String {
    let mut parts = vec!["mon", pnp_id, product_code];
    if let Some(tail) = serial.or(connector) {
        parts.push(tail);
    }
    let raw = parts.join("-").to_lowercase();
    let mut key = String::with_capacity(raw.len());
    for c in raw.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if !(c == '-' && key.ends_with('-')) {
            key.push(c);
        }
    }
    key.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TV: &str =
        r"\\?\DISPLAY#SAM721B#5&2031f481&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";

    #[test]
    fn device_path_parts() {
        assert_eq!(connector_hint(TV).as_deref(), Some("UID4352"));
        assert_eq!(hardware_id(TV).as_deref(), Some("SAM721B"));
        assert_eq!(
            instance_id_from_device_path(TV).as_deref(),
            Some(r"DISPLAY\SAM721B\5&2031f481&0&UID4352")
        );
        assert_eq!(connector_hint("no id here"), None);
        assert_eq!(instance_id_from_device_path("garbage"), None);
    }

    #[test]
    fn keys_are_readable_and_safe() {
        assert_eq!(
            monitor_key("SAM", "721B", Some("376965120"), Some("UID4352")),
            "mon-sam-721b-376965120"
        );
        assert_eq!(
            monitor_key("HXM", "40F3", None, Some("UID4353")),
            "mon-hxm-40f3-uid4353"
        );
        assert_eq!(
            monitor_key("AUS", "AA06", Some("S/N 12 34"), None),
            "mon-aus-aa06-s-n-12-34"
        );
        assert_eq!(monitor_key("AUS", "AA06", None, None), "mon-aus-aa06");
    }
}

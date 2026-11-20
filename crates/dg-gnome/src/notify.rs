//! Desktop notifications: the on-screen display after a switch, and errors from the
//! background agent, which has no console (ARCHITECTURE.md §16.3, L2).

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::zvariant::Value;

/// Shows a notification; `timeout_ms` of 0 lets it stay until dismissed. Returns its id.
pub fn notify(summary: &str, body: &str, icon: &str, timeout_ms: i32) -> Result<u32, String> {
    let conn = Connection::session().map_err(|e| e.to_string())?;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    // Not kept in the notification list: a switch is only news for a moment.
    hints.insert("transient", Value::from(timeout_ms > 0));
    hints.insert("category", Value::from("device"));
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "DisplayGroups",
                0u32,
                icon,
                summary,
                body,
                Vec::<&str>::new(),
                hints,
                timeout_ms,
            ),
        )
        .map_err(|e| e.to_string())?;
    reply.body().deserialize::<u32>().map_err(|e| e.to_string())
}

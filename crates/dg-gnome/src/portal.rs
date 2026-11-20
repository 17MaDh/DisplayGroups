//! The desktop's own "Open file" dialog through xdg-desktop-portal
//! (`org.freedesktop.portal.FileChooser`), the way sandboxed and Wayland apps ask for a
//! file. GNOME shows its regular file chooser; the app only learns the file picked.

use std::collections::HashMap;
use std::path::PathBuf;

use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::desktop::path_from_uri;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.FileChooser";

fn responses(conn: &Connection, path: &str) -> Result<MessageIterator, String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")
        .and_then(|b| b.member("Response"))
        .and_then(|b| b.path(path.to_string()))
        .map_err(|e| e.to_string())?
        .build();
    MessageIterator::for_match_rule(rule, conn, Some(4)).map_err(|e| e.to_string())
}

/// Asks the user for one file of the given media types (`image/png`, …). Blocks until
/// the dialog closes; `Ok(None)` when it was cancelled.
pub fn open_file(
    title: &str,
    filter_name: &str,
    mime_types: &[&str],
) -> Result<Option<PathBuf>, String> {
    let unavailable = |e: String| format!("the file chooser is not available ({e})");
    let conn = Connection::session().map_err(|e| unavailable(e.to_string()))?;
    let sender = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .ok_or_else(|| unavailable("no bus name".into()))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    let token = format!("displaygroups_{}_{nanos}", std::process::id());
    // Listen before asking, so the answer cannot be missed (portal API 0.9 and later
    // make the request's path predictable from the token).
    let expected = format!("{PATH}/request/{sender}/{token}");
    let mut answers = responses(&conn, &expected)?;

    let filter: (String, Vec<(u32, String)>) = (
        filter_name.to_string(),
        mime_types
            .iter()
            .map(|t| (1u32, (*t).to_string()))
            .collect(),
    );
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("modal", Value::from(true));
    options.insert("multiple", Value::from(false));
    options.insert("filters", Value::from(vec![filter.clone()]));
    options.insert("current_filter", Value::from(filter));
    let reply = conn
        .call_method(
            Some(DEST),
            PATH,
            Some(IFACE),
            "OpenFile",
            &("", title, options),
        )
        .map_err(|e| unavailable(e.to_string()))?;
    let handle: OwnedObjectPath = reply.body().deserialize().map_err(|e| e.to_string())?;
    if handle.as_str() != expected {
        answers = responses(&conn, handle.as_str())?;
    }
    let message = answers
        .next()
        .ok_or("the file chooser closed without an answer")?
        .map_err(|e| e.to_string())?;
    let (code, results): (u32, HashMap<String, OwnedValue>) =
        message.body().deserialize().map_err(|e| e.to_string())?;
    // 0 is a choice, 1 cancelled, 2 closed some other way.
    if code != 0 {
        return Ok(None);
    }
    Ok(first_uri(&results).and_then(|uri| path_from_uri(&uri)))
}

fn first_uri(results: &HashMap<String, OwnedValue>) -> Option<String> {
    let Value::Array(uris) = &**results.get("uris")? else {
        return None;
    };
    uris.iter()
        .find_map(|v| <&str>::try_from(v).ok().map(str::to_string))
}

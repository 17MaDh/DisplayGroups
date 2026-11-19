//! Command code shared by every platform: finding and checking groups, saving a
//! capture, printing the list and status, links, actions and the switch record. The
//! platform's `commands` module does the display work (Windows CCD or GNOME Mutter).

use std::time::{SystemTime, UNIX_EPOCH};

use dg_engine::{Captured, PrimaryInfo, Status};
use dg_model::validate::{self, issues_for_group, Severity};
use dg_model::{store, Action, Config, Group};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::output::Output;
use crate::uri::{parse_uri, UriCommand};
use crate::{CliError, Ctx, EXIT_OK};

/// What started a switch; URI switches are restricted (ARCHITECTURE.md §11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Trigger {
    Cli,
    Uri,
    Hotkey,
    Tray,
    Fallback,
    /// Automatic switching (ARCHITECTURE.md §15.1).
    Auto,
}

/// Minimum gap between two `displaygroups://` switches.
const URI_MIN_INTERVAL_SECS: u64 = 3;

pub(crate) fn find<'a>(cfg: &'a Config, key: &str) -> Result<&'a Group, CliError> {
    if cfg.groups.is_empty() {
        return Err(CliError::Config(
            "There are no groups yet. Arrange your displays the way you like, then run:\n  \
             displaygroups capture \"Desktop\""
                .into(),
        ));
    }
    cfg.find_group(key).ok_or_else(|| {
        let names: Vec<&str> = cfg.groups.iter().map(|g| g.name.as_str()).collect();
        CliError::Usage(format!("no group \"{key}\". Groups: {}", names.join(", ")))
    })
}

/// Refuses to apply a group that has validation errors.
pub(crate) fn ensure_valid(cfg: &Config, group: &Group) -> Result<(), CliError> {
    let issues = validate::validate(cfg);
    let index = cfg
        .groups
        .iter()
        .position(|g| g.id == group.id)
        .unwrap_or(usize::MAX);
    let errors: Vec<String> = issues_for_group(&issues, index)
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CliError::Config(format!(
            "\"{}\" cannot be applied:\n{}",
            group.name,
            errors.join("\n")
        )))
    }
}

/// Prints validation warnings for one group.
pub(crate) fn warn_issues(cfg: &Config, id: Uuid, out: Output) {
    let issues = validate::validate(cfg);
    if let Some(index) = cfg.groups.iter().position(|g| g.id == id) {
        for issue in issues_for_group(&issues, index) {
            out.warn(issue.to_string());
        }
    }
}

/// Saves a captured layout as the group `name` (created, or its displays replaced).
pub(crate) fn store_capture(
    ctx: &Ctx,
    cfg: &mut Config,
    name: &str,
    captured: Captured,
    out: Output,
) -> Result<i32, CliError> {
    for (key, monitor) in captured.new_monitors {
        out.line(format!("New monitor: {} ({key})", monitor.display_name));
        cfg.monitors.insert(key, monitor);
    }
    let id = match cfg.find_group(name).map(|g| g.id) {
        Some(id) => {
            let g = cfg.group_by_id_mut(id).expect("found above");
            g.displays = captured.displays;
            out.line(format!("Updated \"{}\".", g.name));
            id
        }
        None => {
            let mut g = Group::new(name);
            g.displays = captured.displays;
            let id = g.id;
            cfg.groups.push(g);
            out.line(format!("Created \"{name}\"."));
            id
        }
    };
    warn_issues(cfg, id, out);
    store::save(&ctx.config_path, cfg)?;

    let group = cfg.group_by_id(id).expect("just inserted");
    for d in &group.displays {
        let name = cfg
            .monitors
            .get(&d.monitor)
            .map_or(d.monitor.as_str(), |m| m.display_name.as_str());
        out.line(format!(
            "  {name}: {}×{} at {} Hz, position ({}, {}){}{}",
            d.mode.width,
            d.mode.height,
            hz(d.mode.refresh_milli_hz),
            d.position.x,
            d.position.y,
            d.scale_percent
                .map(|s| format!(", scale {s}%"))
                .unwrap_or_default(),
            if d.position.x == 0 && d.position.y == 0 {
                ", primary"
            } else {
                ""
            },
        ));
    }
    out.line(format!("id: {id}"));
    out.json(group);
    Ok(EXIT_OK)
}

pub(crate) fn print_list(
    cfg: &Config,
    status: Option<&Status>,
    out: Output,
) -> Result<i32, CliError> {
    out.line("Groups:");
    if cfg.groups.is_empty() {
        out.line("  (none yet; run: displaygroups capture \"Desktop\")");
    }
    for g in &cfg.groups {
        let names: Vec<String> = g
            .enabled_displays()
            .map(|d| {
                let n = cfg
                    .monitors
                    .get(&d.monitor)
                    .map_or(d.monitor.clone(), |m| m.display_name.clone());
                if d.position.x == 0 && d.position.y == 0 {
                    format!("{n}*")
                } else {
                    n
                }
            })
            .collect();
        out.line(format!("  {}  [{}]  {}", g.name, names.join(", "), g.id));
    }
    out.line("Monitors (* = primary in that group):");
    for (key, m) in &cfg.monitors {
        let state = status.map_or("unknown", |s| {
            match s
                .connected
                .iter()
                .find(|c| c.monitor_key.as_deref() == Some(key))
            {
                Some(c) if c.active => "on",
                Some(_) => "connected, off",
                None => "not connected",
            }
        });
        out.line(format!("  {}  ({state})  {key}", m.display_name));
    }
    if let Some(s) = status {
        for c in s.connected.iter().filter(|c| c.monitor_key.is_none()) {
            out.line(format!("  {}  (connected, not in any group yet)", c.name));
        }
    }
    #[derive(Serialize)]
    struct Listing<'a> {
        groups: &'a [Group],
        status: Option<&'a Status>,
    }
    out.json(&Listing {
        groups: &cfg.groups,
        status,
    });
    Ok(EXIT_OK)
}

pub(crate) fn print_status(
    cfg: &Config,
    status: &Status,
    current: Option<Uuid>,
    out: Output,
) -> Result<i32, CliError> {
    out.line(match current.and_then(|id| cfg.group_by_id(id)) {
        Some(g) => format!("Current group: {}", g.name),
        None => "Current group: none matches the screen".to_string(),
    });
    for d in &status.displays {
        out.line(format!(
            "  {}{}: {}×{} at {} Hz, position ({}, {})",
            d.name,
            if d.primary { " (primary)" } else { "" },
            d.width,
            d.height,
            d.refresh_milli_hz.map_or("?".into(), hz),
            d.x,
            d.y
        ));
    }
    for c in status.connected.iter().filter(|c| !c.active) {
        out.line(format!("  {}: connected, off", c.name));
    }
    out.json(status);
    Ok(EXIT_OK)
}

pub(crate) fn duplicate(
    ctx: &Ctx,
    cfg: &mut Config,
    key: &str,
    out: Output,
) -> Result<i32, CliError> {
    let id = find(cfg, key)?.id;
    let new_id = cfg.duplicate_group(id)?;
    store::save(&ctx.config_path, cfg)?;
    let g = cfg.group_by_id(new_id).expect("just created");
    out.line(format!("Created \"{}\" ({new_id}).", g.name));
    out.json(g);
    Ok(EXIT_OK)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UriStamp {
    at_unix: u64,
}

pub(crate) fn handle_uri(ctx: &Ctx, cfg: &Config, url: &str, out: Output) -> Result<i32, CliError> {
    let command = parse_uri(url).map_err(|e| CliError::Usage(format!("rejected link: {e}")))?;
    let now = unix_now();
    let stamp_file = ctx.paths.uri_stamp_file();
    if let Ok(Some(last)) = store::read_json::<UriStamp>(&stamp_file) {
        if now.saturating_sub(last.at_unix) < URI_MIN_INTERVAL_SECS {
            tracing::warn!(
                url,
                "ignored: links are limited to one switch every {URI_MIN_INTERVAL_SECS} s"
            );
            return Ok(EXIT_OK);
        }
    }
    let _ = store::write_json(&stamp_file, &UriStamp { at_unix: now });

    let known = |id: &Uuid| {
        cfg.group_by_id(*id).ok_or_else(|| {
            CliError::Usage(format!(
                "the link points at a group that does not exist ({id})"
            ))
        })
    };
    match command {
        UriCommand::Apply(id) => {
            let group = known(&id)?.clone();
            crate::commands::apply(ctx, cfg, &group, false, Trigger::Uri, out)
        }
        UriCommand::Toggle(ids) => {
            let keys = ids
                .iter()
                .map(|id| known(id).map(|g| g.id.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            crate::commands::toggle(ctx, cfg, &keys, false, Trigger::Uri, out)
        }
    }
}

/// Before/after actions, always in the user's own session.
pub(crate) fn run_actions(actions: &[Action], out: Output) {
    for action in actions {
        let result = match action {
            Action::Program {
                path,
                args,
                working_dir,
            } => crate::sys::spawn(path, args, working_dir.as_deref()),
            Action::Open { target } => crate::sys::open(target),
        };
        match result {
            Ok(()) => tracing::info!(?action, "action started"),
            Err(e) => out.warn(e),
        }
    }
}

/// `state/last-switch.json`: what the agent shows in the OSD.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LastSwitch {
    pub group_id: Uuid,
    pub group_name: String,
    pub icon: String,
    pub primary: Option<PrimaryInfo>,
    pub at_unix: u64,
}

pub(crate) fn write_last_switch(ctx: &Ctx, group: &Group, primary: Option<PrimaryInfo>) {
    let record = LastSwitch {
        group_id: group.id,
        group_name: group.name.clone(),
        icon: group.icon.clone(),
        primary,
        at_unix: unix_now(),
    };
    if let Err(e) = store::write_json(&ctx.paths.last_switch_file(), &record) {
        tracing::warn!("could not record the switch: {e}");
    }
}

pub(crate) fn hz(milli: u32) -> String {
    if milli.is_multiple_of(1000) {
        (milli / 1000).to_string()
    } else {
        format!("{:.2}", f64::from(milli) / 1000.0)
    }
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

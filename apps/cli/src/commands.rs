//! The CLI commands. Pre/post-switch actions run here, in user context only.

use std::time::{SystemTime, UNIX_EPOCH};

use dg_engine::{
    next_in_cycle, ApplyOptions, ApplyReport, Engine, PrimaryInfo, RecoverMethod, Snapshot,
};
use dg_model::validate::{self, issues_for_group, Severity};
use dg_model::{store, Action, Config, Group};
use dg_win::{WinAudio, WinBackend};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::args::Command;
use crate::output::Output;
use crate::uri::{parse_uri, UriCommand};
use crate::{CliError, Ctx, EXIT_OK, EXIT_REVERTED};

/// What started a switch; URI switches are restricted (ARCHITECTURE.md §11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    Cli,
    Uri,
    Hotkey,
    Fallback,
}

/// Minimum gap between two `displaygroups://` switches.
const URI_MIN_INTERVAL_SECS: u64 = 3;

pub(crate) fn dispatch(
    ctx: &Ctx,
    mut cfg: Config,
    command: Command,
    out: Output,
) -> Result<i32, CliError> {
    match command {
        Command::Apply { group, no_confirm } => {
            let group = find(&cfg, &group)?.clone();
            apply(ctx, &cfg, &group, no_confirm, Trigger::Cli, out)
        }
        Command::Toggle { groups, no_confirm } => {
            toggle(ctx, &cfg, &groups, no_confirm, Trigger::Cli, out)
        }
        Command::Capture { group } => capture(ctx, &mut cfg, &group, out),
        Command::Validate { group } => validate_group(&cfg, &group, out),
        Command::List => list(&cfg, out),
        Command::Status => status(&cfg, out),
        Command::Duplicate { group } => duplicate(ctx, &mut cfg, &group, out),
        Command::Recover { extend } => recover(ctx, extend, out),
        Command::Uri { url } => handle_uri(ctx, &cfg, &url, out),
        Command::Agent { dry_run } => crate::agent::run(ctx, cfg, dry_run, out),
    }
}

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
fn ensure_valid(cfg: &Config, group: &Group) -> Result<(), CliError> {
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

pub(crate) fn apply(
    ctx: &Ctx,
    cfg: &Config,
    group: &Group,
    no_confirm: bool,
    trigger: Trigger,
    out: Output,
) -> Result<i32, CliError> {
    ensure_valid(cfg, group)?;
    let display = WinBackend::new();
    let audio = WinAudio::new();
    let engine = Engine::new(&display).with_audio(&audio);
    ensure_lkg(ctx, &display);

    let actions =
        group.actions.enabled && (trigger != Trigger::Uri || group.actions.allow_from_uri);
    if actions {
        run_actions(&group.actions.before, out);
    }

    let confirmer = out.confirmer();
    // Show the OSD as soon as the new layout is visible, before "Keep these settings?".
    let announce = |primary: Option<PrimaryInfo>| {
        write_last_switch(ctx, group, primary);
        dg_win::agent::notify_switched();
    };
    let opts = ApplyOptions {
        confirm_seconds: if no_confirm {
            0
        } else {
            group.safety.confirm_seconds
        },
        confirmer: Some(confirmer.as_ref()),
        on_applied: Some(&announce),
    };
    tracing::info!(group = %group.name, ?trigger, "applying");
    let report = engine.apply(cfg, group, &opts)?;
    tracing::info!(report = ?report, "apply finished");

    if report.kept {
        if let Err(e) = store::write_json(
            &ctx.paths.lkg_file(),
            &Snapshot::now(report.final_state.clone()),
        ) {
            out.warn(format!("could not save the last-known-good layout: {e}"));
        }
        if actions {
            run_actions(&group.actions.after, out);
        }
    }
    print_report(&report, out);
    Ok(if report.kept { EXIT_OK } else { EXIT_REVERTED })
}

/// Saves the current layout as last-known-good the first time, so `recover` always
/// has something to go back to.
fn ensure_lkg(ctx: &Ctx, display: &WinBackend) {
    let path = ctx.paths.lkg_file();
    if path.exists() {
        return;
    }
    if let Ok(active) = dg_engine::DisplayBackend::query(display, dg_engine::ccd::Scope::Active) {
        if let Err(e) = store::write_json(&path, &Snapshot::now(active)) {
            tracing::warn!("could not save the initial layout: {e}");
        }
    }
}

pub(crate) fn toggle(
    ctx: &Ctx,
    cfg: &Config,
    keys: &[String],
    no_confirm: bool,
    trigger: Trigger,
    out: Output,
) -> Result<i32, CliError> {
    let ids = keys
        .iter()
        .map(|k| find(cfg, k).map(|g| g.id))
        .collect::<Result<Vec<_>, _>>()?;
    let display = WinBackend::new();
    let current = Engine::new(&display).current_group(cfg, &ids)?;
    let next = next_in_cycle(&ids, current).expect("at least two groups");
    let group = cfg.group_by_id(next).expect("resolved above").clone();
    tracing::info!(?current, next = %group.name, "toggle");
    apply(ctx, cfg, &group, no_confirm, trigger, out)
}

fn capture(ctx: &Ctx, cfg: &mut Config, name: &str, out: Output) -> Result<i32, CliError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CliError::Usage("give the group a name".into()));
    }
    let display = WinBackend::new();
    let captured = Engine::new(&display).capture(cfg)?;
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

    let issues = validate::validate(cfg);
    let index = cfg
        .groups
        .iter()
        .position(|g| g.id == id)
        .expect("just inserted");
    for issue in issues_for_group(&issues, index) {
        out.warn(issue.to_string());
    }
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

fn validate_group(cfg: &Config, key: &str, out: Output) -> Result<i32, CliError> {
    let group = find(cfg, key)?;
    let issues = validate::validate(cfg);
    let index = cfg
        .groups
        .iter()
        .position(|g| g.id == group.id)
        .expect("found");
    for issue in issues_for_group(&issues, index) {
        out.warn(issue.to_string());
    }
    let display = WinBackend::new();
    let report = Engine::new(&display).validate(cfg, group)?;
    out.line(format!("\"{}\" (nothing was changed):", group.name));
    for a in &report.attempts {
        let verdict = if a.validate == 0 {
            "accepted".to_string()
        } else {
            format!("rejected (Windows error {})", a.validate)
        };
        out.line(format!("  {}: {verdict}", a.strategy));
    }
    match report.would_use {
        Some(s) => out.line(format!("Applying would use {s}.")),
        None => out.line("Windows would reject this group right now."),
    }
    out.json(&report);
    Ok(if report.would_use.is_some() {
        EXIT_OK
    } else {
        6
    })
}

fn list(cfg: &Config, out: Output) -> Result<i32, CliError> {
    let display = WinBackend::new();
    let status = Engine::new(&display).status(cfg).ok();
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
        let state = status.as_ref().map_or("unknown", |s| {
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
    if let Some(s) = &status {
        for c in s.connected.iter().filter(|c| c.monitor_key.is_none()) {
            out.line(format!("  {}  (connected, not in any group yet)", c.name));
        }
    }
    #[derive(Serialize)]
    struct Listing<'a> {
        groups: &'a [Group],
        status: Option<&'a dg_engine::Status>,
    }
    out.json(&Listing {
        groups: &cfg.groups,
        status: status.as_ref(),
    });
    Ok(EXIT_OK)
}

fn status(cfg: &Config, out: Output) -> Result<i32, CliError> {
    let display = WinBackend::new();
    let engine = Engine::new(&display);
    let status = engine.status(cfg)?;
    let ids: Vec<Uuid> = cfg.groups.iter().map(|g| g.id).collect();
    let current = engine
        .current_group(cfg, &ids)?
        .and_then(|id| cfg.group_by_id(id));
    out.line(match current {
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
    out.json(&status);
    Ok(EXIT_OK)
}

fn duplicate(ctx: &Ctx, cfg: &mut Config, key: &str, out: Output) -> Result<i32, CliError> {
    let id = find(cfg, key)?.id;
    let new_id = cfg.duplicate_group(id)?;
    store::save(&ctx.config_path, cfg)?;
    let g = cfg.group_by_id(new_id).expect("just created");
    out.line(format!("Created \"{}\" ({new_id}).", g.name));
    out.json(g);
    Ok(EXIT_OK)
}

pub(crate) fn recover(ctx: &Ctx, extend: bool, out: Output) -> Result<i32, CliError> {
    let snapshot: Option<Snapshot> = store::read_json(&ctx.paths.lkg_file())?;
    let display = WinBackend::new();
    let method = Engine::new(&display).recover(snapshot.as_ref(), extend)?;
    out.line(match method {
        RecoverMethod::LastKnownGood => "Restored the last layout that worked.",
        RecoverMethod::LastKnownGoodTopology => {
            "Restored the last working set of displays (Windows chose the modes)."
        }
        RecoverMethod::Extend => "Extended the desktop across all connected displays.",
    });
    out.json(&method);
    Ok(EXIT_OK)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UriStamp {
    at_unix: u64,
}

fn handle_uri(ctx: &Ctx, cfg: &Config, url: &str, out: Output) -> Result<i32, CliError> {
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
            apply(ctx, cfg, &group, false, Trigger::Uri, out)
        }
        UriCommand::Toggle(ids) => {
            let keys = ids
                .iter()
                .map(|id| known(id).map(|g| g.id.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            toggle(ctx, cfg, &keys, false, Trigger::Uri, out)
        }
    }
}

fn run_actions(actions: &[Action], out: Output) {
    for action in actions {
        let result = match action {
            Action::Program {
                path,
                args,
                working_dir,
            } => dg_win::shell::spawn(path, args, working_dir.as_deref()),
            Action::Open { target } => dg_win::shell::open(target),
        };
        match result {
            Ok(()) => tracing::info!(?action, "action started"),
            Err(e) => out.warn(e),
        }
    }
}

/// `state\last-switch.json`: what the agent shows in the OSD.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LastSwitch {
    pub group_id: Uuid,
    pub group_name: String,
    pub icon: String,
    pub primary: Option<PrimaryInfo>,
    pub at_unix: u64,
}

fn write_last_switch(ctx: &Ctx, group: &Group, primary: Option<PrimaryInfo>) {
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

fn print_report(report: &ApplyReport, out: Output) {
    if report.kept {
        let how = report
            .strategy
            .map(|s| format!(" using {s}"))
            .unwrap_or_default();
        out.line(format!("Switched to \"{}\"{how}.", report.group_name));
        if let Some(p) = report.primary {
            out.line(format!(
                "  Primary: {}×{} at {} Hz",
                p.width,
                p.height,
                p.refresh_milli_hz.map_or("?".into(), hz)
            ));
        }
    } else {
        out.line(format!(
            "Not kept: back to the previous layout (\"{}\" was not confirmed).",
            report.group_name
        ));
    }
    for note in &report.notes {
        out.line(format!("  - {note}"));
    }
    out.json(report);
}

pub(crate) fn hz(milli: u32) -> String {
    if milli.is_multiple_of(1000) {
        (milli / 1000).to_string()
    } else {
        format!("{:.2}", f64::from(milli) / 1000.0)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

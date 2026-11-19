//! The CLI commands on Windows (CCD through `dg-engine` and `dg-win`). Pre/post-switch
//! actions run here, in user context only. Code shared with Linux is in `common`.

use dg_engine::{
    next_in_cycle, ApplyOptions, ApplyReport, Engine, PrimaryInfo, RecoverMethod, Snapshot,
};
use dg_model::{store, Config, Group};
use dg_win::{WinAudio, WinBackend};
use serde::Serialize;
use uuid::Uuid;

use crate::args::Command;
use crate::common::{
    duplicate, ensure_valid, handle_uri, run_actions, warn_issues, write_last_switch,
};
pub(crate) use crate::common::{find, hz, LastSwitch, Trigger};
use crate::output::Output;
use crate::{CliError, Ctx, EXIT_OK, EXIT_REVERTED};

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
        Command::List => {
            let display = WinBackend::new();
            let status = Engine::new(&display).status(&cfg).ok();
            crate::common::print_list(&cfg, status.as_ref(), out)
        }
        Command::Status => status(&cfg, out),
        Command::Identify { seconds } => identify(&cfg, seconds, out),
        Command::Duplicate { group } => duplicate(ctx, &mut cfg, &group, out),
        Command::Recover { extend } => recover(ctx, extend, out),
        Command::Uri { url } => handle_uri(ctx, &cfg, &url, out),
        Command::Shortcut {
            group,
            desktop,
            start_menu,
            icon,
        } => crate::integrate::shortcut(
            ctx,
            &mut cfg,
            &group,
            desktop,
            start_menu,
            icon.as_deref(),
            out,
        ),
        Command::Startup { state } => crate::integrate::startup(state, out),
        Command::Protocol { state } => crate::integrate::protocol(state, out),
        Command::Agent { dry_run } => crate::agent::run(ctx, cfg, dry_run, out),
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
    crate::common::store_capture(ctx, cfg, name, captured, out)
}

fn validate_group(cfg: &Config, key: &str, out: Output) -> Result<i32, CliError> {
    let group = find(cfg, key)?;
    warn_issues(cfg, group.id, out);
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

fn status(cfg: &Config, out: Output) -> Result<i32, CliError> {
    let display = WinBackend::new();
    let engine = Engine::new(&display);
    let status = engine.status(cfg)?;
    let ids: Vec<Uuid> = cfg.groups.iter().map(|g| g.id).collect();
    let current = engine.current_group(cfg, &ids)?;
    crate::common::print_status(cfg, &status, current, out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentifiedDisplay<'a> {
    number: u32,
    name: &'a str,
    monitor_key: Option<&'a str>,
    detail: &'a str,
}

/// Shows each active display's number and name on it. Monitors in the config keep
/// their number from [`Config::monitor_number`], as in the settings app; displays not
/// in the config are numbered after them. Nothing is changed.
fn identify(cfg: &Config, seconds: u32, out: Output) -> Result<i32, CliError> {
    let display = WinBackend::new();
    let engine = Engine::new(&display);
    let status = engine.status(cfg)?;
    let mut next_unknown = u32::try_from(cfg.monitors.len()).unwrap_or(u32::MAX);
    let labels: Vec<dg_win::identify::IdentifyLabel> = status
        .displays
        .iter()
        .map(|d| {
            let key = d.monitor_key.as_deref();
            let number = key.and_then(|k| cfg.monitor_number(k)).unwrap_or_else(|| {
                next_unknown += 1;
                next_unknown
            });
            let title = key
                .and_then(|k| cfg.monitors.get(k))
                .map_or_else(|| d.name.clone(), |m| m.display_name.clone());
            let rate = d
                .refresh_milli_hz
                .map(|m| format!(" · {} Hz", hz(m)))
                .unwrap_or_default();
            dg_win::identify::IdentifyLabel {
                number,
                title,
                detail: format!("{} × {}{rate}", d.width, d.height),
                x: d.x,
                y: d.y,
                width: d.width,
                height: d.height,
            }
        })
        .collect();
    for l in &labels {
        out.line(format!("{}  {} ({})", l.number, l.title, l.detail));
    }
    let listed: Vec<IdentifiedDisplay> = labels
        .iter()
        .zip(&status.displays)
        .map(|(l, d)| IdentifiedDisplay {
            number: l.number,
            name: &l.title,
            monitor_key: d.monitor_key.as_deref(),
            detail: &l.detail,
        })
        .collect();
    out.json(&listed);
    dg_win::identify::show(&labels, seconds * 1000, out.dark, out.rtl).map_err(CliError::Config)?;
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

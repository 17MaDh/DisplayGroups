//! The CLI commands on Linux: GNOME's display service through `dg-gnome`
//! (ARCHITECTURE.md §16). Code shared with Windows is in `common`.
//!
//! GNOME applies a whole layout at once and checks it first. With a confirmation
//! countdown, the change is "persistent": GNOME Shell itself asks "Keep these display
//! settings?" and goes back after 20 seconds without an answer. Without one (`--no-confirm`
//! or a countdown of 0) it is "temporary": applied at once, and GNOME's remembered
//! layout comes back at the next sign-in or display hot-plug.

use std::path::{Path, PathBuf};

use dg_engine::{next_in_cycle, PrimaryInfo};
use dg_gnome::dbus::{Method, Mutter};
use dg_gnome::desktop::{self, Dirs};
use dg_gnome::{ApplyConfig, DisplayState};
use dg_model::{store, Config, Group};
use serde::Serialize;
use uuid::Uuid;

use crate::args::{Command, Toggle};
use crate::common::{
    duplicate, ensure_valid, handle_uri, run_actions, warn_issues, write_last_switch,
};
pub(crate) use crate::common::{find, Trigger};
use crate::output::Output;
use crate::{CliError, Ctx, EXIT_OK};

pub(crate) fn dispatch(
    ctx: &Ctx,
    mut cfg: Config,
    command: Command,
    out: Output,
) -> Result<i32, CliError> {
    let not_yet = |what: &str| {
        Err(CliError::Usage(format!(
            "`{what}` is not available on Linux yet (ARCHITECTURE.md §16)"
        )))
    };
    match command {
        Command::Apply { group, no_confirm } => {
            let group = find(&cfg, &group)?.clone();
            apply(ctx, &cfg, &group, no_confirm, Trigger::Cli, out)
        }
        Command::Toggle { groups, no_confirm } => {
            toggle(ctx, &cfg, &groups, no_confirm, Trigger::Cli, out)
        }
        Command::Capture { group } => {
            let name = group.trim();
            if name.is_empty() {
                return Err(CliError::Usage("give the group a name".into()));
            }
            let state = Mutter::connect()?.state()?;
            let captured = dg_gnome::capture(&cfg, &state)?;
            crate::common::store_capture(ctx, &mut cfg, name, captured, out)
        }
        Command::Validate { group } => validate_group(&cfg, &group, out),
        Command::List => {
            let status = Mutter::connect()
                .and_then(|m| m.state())
                .map(|s| dg_gnome::status(&cfg, &s))
                .ok();
            crate::common::print_list(&cfg, status.as_ref(), out)
        }
        Command::Status => {
            let state = Mutter::connect()?.state()?;
            let status = dg_gnome::status(&cfg, &state);
            let ids: Vec<Uuid> = cfg.groups.iter().map(|g| g.id).collect();
            let current = dg_gnome::current_group(&cfg, &state, &ids);
            crate::common::print_status(&cfg, &status, current, out)
        }
        Command::Duplicate { group } => duplicate(ctx, &mut cfg, &group, out),
        Command::Recover { extend } => recover(ctx, extend, out),
        Command::Uri { url } => handle_uri(ctx, &cfg, &url, out),
        // On GNOME, labels on each screen need the settings app's windows (phase L3).
        Command::Identify { .. } => not_yet("identify"),
        Command::Shortcut {
            group,
            desktop,
            start_menu,
            icon,
        } => shortcut(
            ctx,
            &mut cfg,
            &group,
            desktop,
            start_menu,
            icon.as_deref(),
            out,
        ),
        Command::Startup { state } => startup(state, out),
        Command::Protocol { state } => protocol(state, out),
        Command::Agent { dry_run } => crate::linux_agent::run(ctx, cfg, dry_run, out),
    }
}

/// This program's path, for desktop entries and GNOME shortcuts. An AppImage runs from
/// a temporary mount, so its own file (`$APPIMAGE`) is what must be launched.
pub(crate) fn self_exe() -> Result<PathBuf, CliError> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe().map_err(|e| CliError::Config(e.to_string()))
}

fn dirs() -> Result<Dirs, CliError> {
    Dirs::from_env().ok_or_else(|| CliError::Config("HOME is not set".into()))
}

fn startup(state: Toggle, out: Output) -> Result<i32, CliError> {
    let dirs = dirs()?;
    match state {
        Toggle::On => {
            desktop::set_autostart(&dirs, Some(&self_exe()?)).map_err(CliError::Config)?;
            out.line("The agent will start when you log in.");
        }
        Toggle::Off => {
            desktop::set_autostart(&dirs, None).map_err(CliError::Config)?;
            out.line("The agent will no longer start when you log in.");
        }
        Toggle::Status => out.line(if desktop::autostart_enabled(&dirs) {
            "Start at login: on"
        } else {
            "Start at login: off"
        }),
    }
    Ok(EXIT_OK)
}

fn protocol(state: Toggle, out: Output) -> Result<i32, CliError> {
    let dirs = dirs()?;
    match state {
        Toggle::On => {
            desktop::register_scheme(&dirs, &self_exe()?).map_err(CliError::Config)?;
            out.line("displaygroups:// links now open DisplayGroups (this user only).");
        }
        Toggle::Off => {
            desktop::unregister_scheme(&dirs).map_err(CliError::Config)?;
            out.line("displaygroups:// links are no longer handled.");
        }
        Toggle::Status => match desktop::scheme_command(&dirs) {
            Some(cmd) => out.line(format!("displaygroups:// is handled by: {cmd}")),
            None => out.line("displaygroups:// is not registered."),
        },
    }
    Ok(EXIT_OK)
}

/// The icon a desktop entry shows for a group: a theme icon for built-in icons, the
/// file itself for custom ones.
fn entry_icon(ctx: &Ctx, icon: &str) -> String {
    match icon.strip_prefix("file:") {
        Some(name) => ctx
            .paths
            .icons_dir
            .join(Path::new(name).file_name().unwrap_or_default())
            .to_string_lossy()
            .into_owned(),
        None => desktop::theme_icon(icon.strip_prefix("builtin:").unwrap_or("monitor")).into(),
    }
}

/// `--icon`: a built-in name, or a picture (.png, .svg, .ico) copied into the icons folder.
fn import_icon(ctx: &Ctx, value: &str) -> Result<String, CliError> {
    const BUILTIN: [&str; 5] = ["monitor", "couch", "dual", "gaming", "work"];
    if let Some(name) = value.strip_prefix("builtin:") {
        if !BUILTIN.contains(&name) {
            return Err(CliError::Usage(format!(
                "unknown built-in icon \"{name}\"; use one of {}",
                BUILTIN.join(", ")
            )));
        }
        return Ok(value.to_string());
    }
    let source = Path::new(value);
    let file = source
        .file_name()
        .map(|n| format!("custom-{}", n.to_string_lossy()))
        .ok_or_else(|| CliError::Usage(format!("{value} is not a file")))?;
    let bytes = std::fs::read(source).map_err(|e| CliError::Usage(format!("{value}: {e}")))?;
    store::write_atomic(&ctx.paths.icons_dir.join(&file), &bytes)?;
    Ok(format!("file:{file}"))
}

fn shortcut(
    ctx: &Ctx,
    cfg: &mut Config,
    key: &str,
    on_desktop: bool,
    in_apps: bool,
    icon: Option<&str>,
    out: Output,
) -> Result<i32, CliError> {
    let id = find(cfg, key)?.id;
    if let Some(value) = icon {
        let new_icon = import_icon(ctx, value)?;
        let group = cfg.group_by_id_mut(id).expect("found above");
        if group.icon != new_icon {
            group.icon = new_icon;
            store::save(&ctx.config_path, cfg)?;
        }
    }
    let group = cfg.group_by_id(id).expect("found above");
    let dirs = dirs()?;
    let text = desktop::launcher_entry(
        &self_exe()?,
        &group.id.to_string(),
        &group.name,
        &entry_icon(ctx, &group.icon),
    );
    let file = desktop::launcher_file_name(&group.id.to_string());
    let places = match (on_desktop, in_apps) {
        (false, true) => vec![&dirs.applications],
        (true, true) => vec![&dirs.desktop, &dirs.applications],
        _ => vec![&dirs.desktop],
    };
    for dir in places {
        let path = desktop::write_launcher(dir, &file, &text).map_err(CliError::Config)?;
        out.line(format!("Created {}", path.display()));
    }
    if in_apps {
        out.line("It is in the app grid: right-click it and choose \"Pin to Dash\" to keep it in the dock.");
    }
    Ok(EXIT_OK)
}

/// The on-screen display after a switch: a short desktop notification with the group
/// and the new primary's mode, unless the OSD is turned off.
pub(crate) fn announce(ctx: &Ctx, cfg: &Config, title: &str, detail: &str, icon: &str) {
    let osd = &cfg.settings.osd;
    if !osd.enabled {
        return;
    }
    let timeout = i32::try_from(osd.duration_ms.max(1000)).unwrap_or(i32::MAX);
    if let Err(e) = dg_gnome::notify::notify(title, detail, &entry_icon(ctx, icon), timeout) {
        tracing::info!("no notification: {e}");
    }
}

pub(crate) fn mode_text(p: Option<PrimaryInfo>) -> String {
    p.map(|p| {
        let rate = p
            .refresh_milli_hz
            .map(|m| format!(" · {} Hz", crate::common::hz(m)))
            .unwrap_or_default();
        format!("{} × {}{rate}", p.width, p.height)
    })
    .unwrap_or_default()
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
    let mutter = Mutter::connect()?;
    let state = mutter.state()?;
    let plan = dg_gnome::plan(cfg, group, &state)?;
    // GNOME checks the whole layout first; nothing changes if it refuses.
    mutter.apply(&plan, Method::Verify)?;

    let actions =
        group.actions.enabled && (trigger != Trigger::Uri || group.actions.allow_from_uri);
    if actions {
        run_actions(&group.actions.before, out);
    }
    let ask = !no_confirm && group.safety.confirm_seconds > 0;
    let method = if ask {
        Method::Persistent
    } else {
        Method::Temporary
    };
    tracing::info!(group = %group.name, ?trigger, ?method, plan = ?plan, "applying");
    mutter.apply(&plan, method)?;
    let primary = primary_of(&plan, &state);
    write_last_switch(ctx, group, primary);
    announce(ctx, cfg, &group.name, &mode_text(primary), &group.icon);
    if let Err(e) = store::write_json(&ctx.paths.lkg_file(), &plan) {
        out.warn(format!("could not save the last layout: {e}"));
    }
    if actions {
        run_actions(&group.actions.after, out);
    }

    out.line(format!("Switched to \"{}\".", group.name));
    if ask {
        out.line(
            "  GNOME asks whether to keep it, and goes back after 20 seconds without an answer.",
        );
    }
    for note in &plan.notes {
        out.line(format!("  - {note}"));
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Report<'a> {
        group_id: Uuid,
        group_name: &'a str,
        method: &'a str,
        plan: &'a ApplyConfig,
    }
    out.json(&Report {
        group_id: group.id,
        group_name: &group.name,
        method: if ask { "persistent" } else { "temporary" },
        plan: &plan,
    });
    Ok(EXIT_OK)
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
    let state = Mutter::connect()?.state()?;
    let current = dg_gnome::current_group(cfg, &state, &ids);
    let next = next_in_cycle(&ids, current).expect("at least two groups");
    let group = cfg.group_by_id(next).expect("resolved above").clone();
    tracing::info!(?current, next = %group.name, "toggle");
    apply(ctx, cfg, &group, no_confirm, trigger, out)
}

fn validate_group(cfg: &Config, key: &str, out: Output) -> Result<i32, CliError> {
    let group = find(cfg, key)?;
    warn_issues(cfg, group.id, out);
    let mutter = Mutter::connect()?;
    let state = mutter.state()?;
    let plan = dg_gnome::plan(cfg, group, &state)?;
    let verdict = mutter.apply(&plan, Method::Verify);
    out.line(format!("\"{}\" (nothing was changed):", group.name));
    for l in &plan.logical_monitors {
        for (connector, mode) in &l.monitors {
            out.line(format!(
                "  {connector}: {mode} at ({}, {}), scale {}%{}",
                l.x,
                l.y,
                (l.scale * 100.0).round(),
                if l.primary { ", primary" } else { "" }
            ));
        }
    }
    for note in &plan.notes {
        out.line(format!("  - {note}"));
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Report<'a> {
        group_id: Uuid,
        group_name: &'a str,
        /// The settings app reads this field (as on Windows, where it names a strategy).
        would_use: Option<&'a str>,
        rejected: Option<String>,
        plan: &'a ApplyConfig,
    }
    let (would_use, rejected, code) = match verdict {
        Ok(()) => {
            out.line("GNOME accepts this layout.");
            (Some("GNOME"), None, EXIT_OK)
        }
        Err(e) => {
            out.line(format!("GNOME would reject this group right now: {e}"));
            (None, Some(e.to_string()), 6)
        }
    };
    out.json(&Report {
        group_id: group.id,
        group_name: &group.name,
        would_use,
        rejected,
        plan: &plan,
    });
    Ok(code)
}

/// Re-applies the last layout DisplayGroups applied, or turns every connected display
/// on side by side (`--extend`, or when there is no last layout). Applied without
/// GNOME's question, since the screen may be hard to read.
pub(crate) fn recover(ctx: &Ctx, extend: bool, out: Output) -> Result<i32, CliError> {
    let mutter = Mutter::connect()?;
    let state = mutter.state()?;
    let last: Option<ApplyConfig> = if extend {
        None
    } else {
        store::read_json(&ctx.paths.lkg_file())?
    };
    let (plan, what) = match last {
        Some(mut p) => {
            p.serial = state.serial;
            (p, "Restored the last layout DisplayGroups applied.")
        }
        None => (
            dg_gnome::extend_all(&state),
            "Turned on every connected display, side by side.",
        ),
    };
    match mutter.apply(&plan, Method::Temporary) {
        Ok(()) => {}
        Err(e) if !extend => {
            out.warn(format!(
                "the last layout no longer fits ({e}); extending instead"
            ));
            let state = mutter.state()?;
            mutter.apply(&dg_gnome::extend_all(&state), Method::Temporary)?;
            out.line("Turned on every connected display, side by side.");
            return Ok(EXIT_OK);
        }
        Err(e) => return Err(e.into()),
    }
    out.line(what);
    out.json(&plan);
    Ok(EXIT_OK)
}

/// The new primary's mode, for the switch record.
fn primary_of(plan: &ApplyConfig, state: &DisplayState) -> Option<PrimaryInfo> {
    let l = plan.logical_monitors.iter().find(|l| l.primary)?;
    let (connector, mode_id) = l.monitors.first()?;
    let mode = state
        .monitor(connector)?
        .modes
        .iter()
        .find(|m| &m.id == mode_id)?;
    let (width, height) = dg_gnome::ops::rotated(mode.width, mode.height, l.transform);
    Some(PrimaryInfo {
        width,
        height,
        refresh_milli_hz: Some(mode.refresh_milli_hz()),
    })
}

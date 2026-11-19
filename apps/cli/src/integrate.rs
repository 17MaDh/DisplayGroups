//! `shortcut`, `startup` and `protocol` commands, plus icon files for groups.

use std::path::{Path, PathBuf};

use dg_model::{store, Config, Group};
use dg_win::icons;
use dg_win::integration::{self, Place, ShortcutSpec};

use crate::args::Toggle;
use crate::commands::find;
use crate::output::Output;
use crate::{CliError, Ctx, EXIT_OK};

/// `displaygroupsw.exe` next to the running executable: shortcuts, the Run key and the
/// protocol handler all point at the console-less binary.
pub(crate) fn windowless_exe() -> Result<PathBuf, CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::Config(e.to_string()))?;
    let path = exe.with_file_name("displaygroupsw.exe");
    if path.is_file() {
        Ok(path)
    } else {
        Err(CliError::Config(format!("{} is missing", path.display())))
    }
}

/// The `.ico` file for a group's icon, generating built-in icons on first use.
pub(crate) fn icon_file(ctx: &Ctx, icon: &str) -> Result<PathBuf, CliError> {
    let dir = &ctx.paths.icons_dir;
    if let Some(name) = icon.strip_prefix("file:") {
        let path = dir.join(Path::new(name).file_name().unwrap_or_default());
        return if path.is_file() {
            Ok(path)
        } else {
            Err(CliError::Config(format!(
                "icon file {} is missing",
                path.display()
            )))
        };
    }
    let name = icon.strip_prefix("builtin:").unwrap_or("monitor");
    let name = if icons::BUILTIN.contains(&name) {
        name
    } else {
        "monitor"
    };
    let path = dir.join(format!("builtin-{name}.ico"));
    if !path.is_file() {
        let ico = icons::render_ico(icons::builtin_style(name)).map_err(CliError::Config)?;
        store::write_atomic(&path, &ico)?;
    }
    Ok(path)
}

/// Turns `--icon` into a value for `group.icon`, importing custom files into the icons folder.
fn import_icon(ctx: &Ctx, value: &str) -> Result<String, CliError> {
    if let Some(name) = value.strip_prefix("builtin:") {
        if !icons::BUILTIN.contains(&name) {
            return Err(CliError::Usage(format!(
                "unknown built-in icon \"{name}\"; use one of {}",
                icons::BUILTIN.join(", ")
            )));
        }
        return Ok(value.to_string());
    }
    let source = Path::new(value);
    let bytes = std::fs::read(source).map_err(|e| CliError::Usage(format!("{value}: {e}")))?;
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ico = match ext.as_str() {
        "ico" => bytes,
        "png" => icons::png_to_ico(&bytes).map_err(CliError::Usage)?,
        _ => {
            return Err(CliError::Usage(
                "the icon must be a .ico or .png file".into(),
            ))
        }
    };
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = format!("custom-{}.ico", integration::safe_file_name(&stem));
    store::write_atomic(&ctx.paths.icons_dir.join(&file), &ico)?;
    Ok(format!("file:{file}"))
}

pub(crate) fn shortcut(
    ctx: &Ctx,
    cfg: &mut Config,
    key: &str,
    desktop: bool,
    start_menu: bool,
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
    let group: &Group = cfg.group_by_id(id).expect("found above");
    let target = windowless_exe()?;
    let icon_path = icon_file(ctx, &group.icon)?;
    let arguments = format!("apply {}", group.id);
    let description = format!("Switch displays to \"{}\" (DisplayGroups)", group.name);
    let places: Vec<Place> = match (desktop, start_menu) {
        (false, false) | (true, false) => vec![Place::Desktop],
        (false, true) => vec![Place::StartMenu],
        (true, true) => vec![Place::Desktop, Place::StartMenu],
    };
    for place in places {
        let dir = integration::folder(place).map_err(CliError::Config)?;
        let path = integration::create_shortcut(
            &dir,
            &ShortcutSpec {
                name: &group.name,
                target: &target,
                arguments: &arguments,
                icon: Some(&icon_path),
                description: &description,
            },
        )
        .map_err(CliError::Config)?;
        out.line(format!("Created {}", path.display()));
    }
    if start_menu {
        out.line("To pin it, right-click the shortcut in Start and choose \"Pin to Start\".");
    }
    Ok(EXIT_OK)
}

pub(crate) fn startup(state: Toggle, out: Output) -> Result<i32, CliError> {
    match state {
        Toggle::On => {
            let exe = windowless_exe()?;
            integration::set_startup(Some(&exe)).map_err(CliError::Config)?;
            out.line("The agent will start when you sign in to Windows.");
        }
        Toggle::Off => {
            integration::set_startup(None).map_err(CliError::Config)?;
            out.line("The agent will no longer start with Windows.");
        }
        Toggle::Status => out.line(if integration::startup_enabled() {
            "Start with Windows: on"
        } else {
            "Start with Windows: off"
        }),
    }
    Ok(EXIT_OK)
}

pub(crate) fn protocol(state: Toggle, out: Output) -> Result<i32, CliError> {
    match state {
        Toggle::On => {
            let exe = windowless_exe()?;
            integration::register_protocol(&exe).map_err(CliError::Config)?;
            out.line("displaygroups:// links now open DisplayGroups (this user only).");
        }
        Toggle::Off => {
            integration::unregister_protocol().map_err(CliError::Config)?;
            out.line("displaygroups:// links are no longer handled.");
        }
        Toggle::Status => match integration::protocol_command() {
            Some(cmd) => out.line(format!("displaygroups:// is handled by: {cmd}")),
            None => out.line("displaygroups:// is not registered."),
        },
    }
    Ok(EXIT_OK)
}

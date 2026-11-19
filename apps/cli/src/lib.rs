//! Shared implementation of `displaygroups.exe` and `displaygroupsw.exe`.

#[cfg(windows)]
mod agent;
mod args;
#[cfg(windows)]
mod commands;
#[cfg(not(windows))]
#[path = "linux.rs"]
mod commands;
mod common;
mod i18n;
#[cfg(windows)]
mod integrate;
mod logging;
mod output;
mod sys;
pub mod uri;

use std::path::{Path, PathBuf};

use clap::Parser;
use dg_engine::EngineError;
use dg_model::paths::AppPaths;
use dg_model::ModelError;

pub use args::{Cli, Command};
use output::Output;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Config(String),
    #[cfg(not(windows))]
    #[error(transparent)]
    Gnome(#[from] dg_gnome::GnomeError),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        #[cfg(not(windows))]
        use dg_gnome::GnomeError;
        match self {
            CliError::Usage(_) => 2,
            CliError::Engine(
                EngineError::MonitorMissing { .. } | EngineError::AmbiguousMonitor { .. },
            ) => 4,
            CliError::Engine(EngineError::Locked) => 5,
            CliError::Engine(EngineError::AllStrategiesFailed { .. }) => 6,
            #[cfg(not(windows))]
            CliError::Gnome(
                GnomeError::MonitorMissing { .. } | GnomeError::AmbiguousMonitor { .. },
            ) => 4,
            #[cfg(not(windows))]
            CliError::Gnome(GnomeError::Rejected(_)) => 6,
            _ => 1,
        }
    }
}

/// Exit codes besides errors.
pub const EXIT_OK: i32 = 0;
pub const EXIT_REVERTED: i32 = 3;

#[derive(Clone)]
pub(crate) struct Ctx {
    pub paths: AppPaths,
    pub config_path: PathBuf,
}

pub fn main_entry(console: bool) -> i32 {
    #[cfg(windows)]
    dg_win::ui::set_dpi_aware();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if console => e.exit(),
        Err(e) => {
            let text = e.to_string();
            if e.use_stderr() {
                sys::show_error("DisplayGroups", &text);
                return 2;
            }
            sys::show_info("DisplayGroups", &text);
            return 0;
        }
    };
    let out = Output {
        console,
        json: cli.json,
        dark: false,
        rtl: false,
    };
    match run(cli, out) {
        Ok(code) => code,
        Err(e) => {
            out.error(e.to_string());
            e.exit_code()
        }
    }
}

/// Resolves the theme setting for native UI.
pub(crate) fn prefers_dark(theme: dg_model::Theme) -> bool {
    match theme {
        dg_model::Theme::Dark => true,
        dg_model::Theme::Light => false,
        dg_model::Theme::System => sys::prefers_dark(),
    }
}

fn run(cli: Cli, out: Output) -> Result<i32, CliError> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let paths = AppPaths::resolve(exe_dir.as_deref())?;
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| paths.config_file.clone());
    let ctx = Ctx { paths, config_path };

    let loaded = dg_model::store::load(&ctx.config_path);
    let level = loaded
        .as_ref()
        .map(|l| l.config.settings.log_level.as_str())
        .unwrap_or("info");
    let _log_guard = logging::init(&ctx.paths.logs_dir, level);
    let loaded = loaded?;
    let settings = &loaded.config.settings;
    i18n::init(&settings.language);
    let out = Output {
        dark: prefers_dark(settings.theme),
        rtl: i18n::is_rtl(),
        ..out
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        command = ?cli.command,
        config = %ctx.config_path.display(),
        "start"
    );
    if let Some(from) = loaded.migrated_from {
        out.warn(format!(
            "config upgraded from schema {from}; a backup was kept next to it"
        ));
    }
    commands::dispatch(&ctx, loaded.config, cli.command, out)
}

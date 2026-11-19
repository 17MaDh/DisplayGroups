use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "displaygroups",
    version,
    about = "Switch between groups of displays (monitors, TVs, projectors).",
    after_help = "Groups are referenced by name (case-insensitive) or by id.\n\
                  Exit codes: 0 ok, 1 error, 2 usage, 3 reverted, 4 monitor missing, 5 Windows locked, 6 rejected by Windows."
)]
pub struct Cli {
    /// Use this config file instead of %APPDATA%\DisplayGroups\config.json.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Switch to a group.
    Apply {
        group: String,
        /// Skip the "Keep these display settings?" countdown.
        #[arg(long, visible_alias = "force")]
        no_confirm: bool,
    },
    /// Switch to the group after the one on screen now (wraps around).
    Toggle {
        #[arg(required = true, num_args = 2..)]
        groups: Vec<String>,
        #[arg(long, visible_alias = "force")]
        no_confirm: bool,
    },
    /// Save the current layout as a group (created or overwritten).
    Capture { group: String },
    /// Ask Windows whether a group can be applied, without changing anything.
    Validate { group: String },
    /// List groups and monitors.
    List,
    /// Show what is on screen now.
    Status,
    /// Copy a group under a new name.
    Duplicate { group: String },
    /// Restore the last layout that worked (or extend all displays with --extend).
    Recover {
        #[arg(long)]
        extend: bool,
    },
    /// Handle a displaygroups:// link (used by the protocol handler).
    Uri { url: String },
    /// Run the background agent: hotkeys, on-screen display, disconnect fallback.
    /// Normally started with Windows through displaygroupsw.exe.
    Agent {
        /// Show what a hotkey would do on the OSD, without switching displays.
        #[arg(long)]
        dry_run: bool,
    },
}

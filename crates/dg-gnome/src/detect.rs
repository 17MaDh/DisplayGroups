//! What automatic switching watches, on Linux (ARCHITECTURE.md §15.1, §16.3): running
//! programs from `/proc` and game controllers from `/sys`. Nothing needs root.
//!
//! Trigger names are Windows executable names (`steam.exe`), and the trigger logic adds
//! `.exe` to names without it. Linux programs are reported the same way (`steam` →
//! `steam.exe`), so one config works on both systems; Proton games already run as
//! `Game.exe`.

use std::collections::HashSet;

/// The lower-case program name of a process from its `/proc/<pid>/cmdline` (the first
/// argument, without its folder) or, for kernel threads, its `comm`.
pub fn program_name(cmdline: &[u8], comm: &str) -> Option<String> {
    let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
    let argv0 = String::from_utf8_lossy(argv0);
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let name = if base.is_empty() { comm.trim() } else { base };
    if name.is_empty() {
        return None;
    }
    let mut name = name.to_lowercase();
    if !name.ends_with(".exe") {
        name.push_str(".exe");
    }
    Some(name)
}

/// Names of the programs running now (this user's and, where readable, others').
pub fn running_program_names() -> HashSet<String> {
    let mut names = HashSet::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return names;
    };
    for entry in dir.flatten() {
        let file_name = entry.file_name();
        if !file_name
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let path = entry.path();
        let cmdline = std::fs::read(path.join("cmdline")).unwrap_or_default();
        let comm = std::fs::read_to_string(path.join("comm")).unwrap_or_default();
        if let Some(name) = program_name(&cmdline, &comm) {
            names.insert(name);
        }
    }
    names
}

/// Programs that run as part of the desktop session itself, never something a trigger
/// would watch for. Their names are compared without `.exe`.
const SESSION_PROGRAMS: [&str; 27] = [
    "systemd",
    "(sd-pam)",
    "sd-pam",
    "dbus-daemon",
    "dbus-broker",
    "dbus-broker-launch",
    "dbus-run-session",
    "gnome-shell",
    "gnome-session-binary",
    "gnome-keyring-daemon",
    "gnome-shell-calendar-server",
    "mutter-x11-frames",
    "xwayland",
    "pipewire",
    "pipewire-pulse",
    "wireplumber",
    "pulseaudio",
    "ibus-daemon",
    "snapd-desktop-integration",
    "ssh-agent",
    "gpg-agent",
    "bash",
    "sh",
    "dash",
    "zsh",
    "fish",
    "sudo",
];

/// The program name to offer when choosing a trigger, or `None` for the session's own
/// services: helpers installed in a `libexec` folder, shells, and [`SESSION_PROGRAMS`].
pub fn listed_program(cmdline: &[u8], comm: &str) -> Option<String> {
    let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
    if String::from_utf8_lossy(argv0).contains("/libexec/") {
        return None;
    }
    let name = program_name(cmdline, comm)?;
    let bare = name.trim_end_matches(".exe");
    let session = SESSION_PROGRAMS.contains(&bare)
        || [
            "gsd-",
            "xdg-",
            "at-spi",
            "evolution-",
            "ibus-",
            "gvfs",
            "tracker-",
            "goa-",
        ]
        .iter()
        .any(|p| bare.starts_with(p))
        || bare.starts_with("displaygroups");
    (!session).then_some(name)
}

/// This user's programs running now, sorted, without the session's own services: the
/// list the settings app offers when choosing a trigger.
#[cfg(unix)]
pub fn user_programs() -> Vec<String> {
    use std::os::unix::fs::MetadataExt;
    let Ok(me) = std::fs::metadata("/proc/self").map(|m| m.uid()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .filter(|e| e.metadata().is_ok_and(|m| m.uid() == me))
        .filter_map(|e| {
            let path = e.path();
            let cmdline = std::fs::read(path.join("cmdline")).ok()?;
            // Kernel threads have no command line.
            if cmdline.is_empty() {
                return None;
            }
            let comm = std::fs::read_to_string(path.join("comm")).unwrap_or_default();
            listed_program(&cmdline, &comm)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Whether a game controller is connected: a joystick device (`/sys/class/input/js*`)
/// or an input device udev tags as a joystick.
pub fn controller_connected() -> bool {
    let joystick = std::fs::read_dir("/sys/class/input").is_ok_and(|dir| {
        dir.flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("js"))
    });
    joystick
        || std::fs::read_dir("/run/udev/data").is_ok_and(|dir| {
            dir.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("c13:"))
                .any(|e| {
                    std::fs::read_to_string(e.path())
                        .is_ok_and(|t| t.lines().any(|l| l == "E:ID_INPUT_JOYSTICK=1"))
                })
        })
}

#[cfg(test)]
mod tests {
    use super::{listed_program, program_name};

    #[test]
    fn session_services_are_not_offered_for_triggers() {
        assert_eq!(
            listed_program(b"/usr/bin/steam\0", "steam").as_deref(),
            Some("steam.exe")
        );
        assert_eq!(
            listed_program(b"Z:\\Games\\Game.exe\0", "Game.exe").as_deref(),
            Some("game.exe")
        );
        assert_eq!(
            listed_program(b"/usr/libexec/gsd-power\0", "gsd-power"),
            None
        );
        assert_eq!(
            listed_program(b"/usr/lib/x86_64-linux-gnu/libexec/kf6/x\0", "x"),
            None
        );
        assert_eq!(
            listed_program(b"/usr/bin/gnome-shell\0", "gnome-shell"),
            None
        );
        assert_eq!(listed_program(b"bash\0", "bash"), None);
        assert_eq!(listed_program(b"/usr/bin/xdg-desktop-portal\0", "x"), None);
        assert_eq!(listed_program(b"./displaygroups\0agent\0", "x"), None);
    }

    #[test]
    fn names_match_windows_trigger_names() {
        assert_eq!(
            program_name(b"/usr/lib/steam/steam\0-silent\0", "steam").as_deref(),
            Some("steam.exe")
        );
        assert_eq!(
            program_name(b"Z:\\games\\Elden Ring\\eldenring.exe\0", "eldenring.exe").as_deref(),
            Some("eldenring.exe")
        );
        assert_eq!(
            program_name(b"", "kworker/0:1").as_deref(),
            Some("kworker/0:1.exe")
        );
        assert_eq!(program_name(b"", "").as_deref(), None);
    }
}

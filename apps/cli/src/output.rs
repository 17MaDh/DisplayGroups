//! Where messages go: the terminal for `displaygroups.exe`, message boxes (errors only)
//! for `displaygroupsw.exe`.

#[cfg(windows)]
use std::io::IsTerminal;
#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use dg_engine::Confirmer;
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Output {
    pub console: bool,
    pub json: bool,
    /// Dark theme for native dialogs (resolved from the config's theme setting).
    pub dark: bool,
    /// Right-to-left layout for native dialogs (from the loaded language).
    pub rtl: bool,
}

impl Output {
    pub fn line(&self, text: impl AsRef<str>) {
        if self.console && !self.json {
            println!("{}", text.as_ref());
        }
    }

    pub fn warn(&self, text: impl AsRef<str>) {
        tracing::warn!("{}", text.as_ref());
        if self.console {
            eprintln!("warning: {}", text.as_ref());
        }
    }

    pub fn json<T: Serialize>(&self, value: &T) {
        if self.console && self.json {
            match serde_json::to_string_pretty(value) {
                Ok(s) => println!("{s}"),
                Err(e) => eprintln!("error: {e}"),
            }
        }
    }

    pub fn error(&self, text: impl AsRef<str>) {
        tracing::error!("{}", text.as_ref());
        if self.console {
            eprintln!("error: {}", text.as_ref());
        } else {
            crate::sys::show_error("DisplayGroups", text.as_ref());
        }
    }

    /// Terminal prompt when a person is at the console; the countdown dialog otherwise
    /// (shortcuts, hotkeys, scripts with redirected input). On GNOME, GNOME Shell asks.
    #[cfg(windows)]
    pub fn confirmer(&self) -> Box<dyn Confirmer> {
        if self.console && std::io::stdin().is_terminal() {
            Box::new(ConsoleConfirmer)
        } else {
            use crate::i18n::t;
            Box::new(dg_win::confirm::CountdownDialog {
                title: t("confirm.title"),
                body: t("confirm.body"),
                keep: t("confirm.keep"),
                revert: t("confirm.revert"),
                dark: self.dark,
                rtl: self.rtl,
            })
        }
    }
}

#[cfg(windows)]
pub struct ConsoleConfirmer;

#[cfg(windows)]
impl Confirmer for ConsoleConfirmer {
    fn confirm(&self, seconds: u32, group_name: &str) -> bool {
        eprintln!(
            "Switched to \"{group_name}\". Keep these display settings? \
             Type y and press Enter within {seconds} s; anything else reverts."
        );
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let _ = tx.send(line);
        });
        match rx.recv_timeout(Duration::from_secs(seconds.into())) {
            Ok(line) => matches!(line.trim().to_lowercase().as_str(), "y" | "yes"),
            Err(_) => {
                eprintln!("No answer; reverting.");
                false
            }
        }
    }
}

//! Windowless entry point for shortcuts and the `displaygroups://` protocol: no console
//! window flashes; errors are shown in a message box.

#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(dg_cli::main_entry(false));
}

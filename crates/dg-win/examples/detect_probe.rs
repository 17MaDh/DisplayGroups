//! What automatic switching sees on this machine, and how long each check takes:
//! `cargo run -p dg-win --example detect_probe [-- --names]`
//!
//! Without `--names` only counts are printed, so the output can be shared in a bug report.

use std::time::Instant;

fn main() {
    let names = std::env::args().any(|a| a == "--names");

    let t = Instant::now();
    let programs = dg_win::detect::running_program_names();
    println!("processes:   {:>4}  ({:?})", programs.len(), t.elapsed());

    let t = Instant::now();
    let windows = dg_win::detect::visible_windows();
    println!("windows:     {:>4}  ({:?})", windows.len(), t.elapsed());

    let t = Instant::now();
    let pickable = dg_win::detect::programs_with_windows();
    println!("pickable:    {:>4}  ({:?})", pickable.len(), t.elapsed());
    if names {
        for name in &pickable {
            println!("  {name}");
        }
    }

    let t = Instant::now();
    let controller = dg_win::detect::controller_connected();
    println!("controller:  {controller}  ({:?})", t.elapsed());
}

//! Console entry point, for terminals and scripts.

fn main() {
    std::process::exit(dg_cli::main_entry(true));
}

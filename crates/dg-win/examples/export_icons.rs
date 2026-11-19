//! Writes every built-in icon to a folder, for review:
//! `cargo run -p dg-win --example export_icons -- <dir>`

fn main() {
    let dir = std::env::args().nth(1).expect("usage: export_icons <dir>");
    std::fs::create_dir_all(&dir).expect("create dir");
    println!("icon font: {}", dg_win::icons::icon_font_face());
    for name in dg_win::icons::BUILTIN {
        let ico = dg_win::icons::render_ico(dg_win::icons::builtin_style(name)).expect("render");
        let path = std::path::Path::new(&dir).join(format!("{name}.ico"));
        std::fs::write(&path, ico).expect("write");
        println!("{}", path.display());
    }
}

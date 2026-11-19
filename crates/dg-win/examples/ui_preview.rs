//! Shows the OSD and the countdown dialog for visual review (nothing is switched):
//! `cargo run -p dg-win --example ui_preview -- [dark|light] [seconds]`

use dg_engine::Confirmer;

fn main() {
    dg_win::ui::set_dpi_aware();
    let dark = std::env::args().nth(1).as_deref() != Some("light");
    let seconds: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    let osd = dg_win::osd::Osd::create().expect("OSD");
    osd.show(
        dg_win::osd::OsdContent {
            glyph: Some('\u{E7F4}'),
            title: "Gaming on Couch".into(),
            detail: "3840 × 2160 · 120 Hz".into(),
            dark,
        },
        (seconds * 1000).max(1800),
    );
    let kept = dg_win::confirm::CountdownDialog::english(dark).confirm(seconds, "Gaming on Couch");
    println!("kept={kept}");
}

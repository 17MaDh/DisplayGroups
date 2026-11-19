//! Shows the OSD and the countdown dialog for visual review (nothing is switched):
//! `cargo run -p dg-win --example ui_preview -- [dark|light] [seconds] [ar]`
//!
//! `ar` uses Arabic sample texts (the same wording as `locales/ar.json`) and the
//! right-to-left layout.

use dg_engine::Confirmer;

fn main() {
    dg_win::ui::set_dpi_aware();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dark = args.first().map(String::as_str) != Some("light");
    let seconds: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let rtl = args.get(2).map(String::as_str) == Some("ar");
    let osd = dg_win::osd::Osd::create().expect("OSD");
    osd.show(
        dg_win::osd::OsdContent {
            glyph: Some('\u{E7F4}'),
            title: if rtl {
                "ألعاب على الأريكة"
            } else {
                "Gaming on Couch"
            }
            .into(),
            detail: "3840 × 2160 · 120 Hz".into(),
            dark,
            rtl,
        },
        (seconds * 1000).max(1800),
    );
    let dialog = if rtl {
        dg_win::confirm::CountdownDialog {
            title: "هل تريد الاحتفاظ بإعدادات العرض هذه؟".into(),
            body: "تم التبديل إلى \"{group}\". ستتم العودة إلى التخطيط السابق خلال {seconds} ث."
                .into(),
            keep: "الاحتفاظ بالتغييرات".into(),
            revert: "تراجع".into(),
            dark,
            rtl,
        }
    } else {
        dg_win::confirm::CountdownDialog::english(dark)
    };
    let group = if rtl {
        "ألعاب على الأريكة"
    } else {
        "Gaming on Couch"
    };
    let kept = dialog.confirm(seconds, group);
    println!("kept={kept}");
}

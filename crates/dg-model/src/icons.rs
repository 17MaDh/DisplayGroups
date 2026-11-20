//! The built-in group icons (`group.icon` = `builtin:<name>`): a glyph on a coloured tile.
//!
//! The glyph is a Segoe Fluent Icons code point. Windows draws it with that font (the
//! OSD, shortcut icons, the settings app); systems without the font, such as Linux, map
//! the same code point to an icon of their own.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconStyle {
    pub glyph: char,
    /// Tile colour (r, g, b).
    pub color: (u8, u8, u8),
}

/// The built-in icon names accepted in `group.icon` as `builtin:<name>`.
pub const BUILTIN: [&str; 5] = ["monitor", "couch", "dual", "gaming", "work"];

/// Style for `builtin:<name>`; unknown names get the monitor style.
pub fn builtin_style(name: &str) -> IconStyle {
    match name {
        "couch" => IconStyle {
            glyph: '\u{E7F4}', // TVMonitor
            color: (124, 58, 237),
        },
        "dual" => IconStyle {
            glyph: '\u{E7F4}',
            color: (13, 148, 136),
        },
        "gaming" => IconStyle {
            glyph: '\u{E7FC}', // Game
            color: (22, 163, 74),
        },
        "work" => IconStyle {
            glyph: '\u{E821}', // Work
            color: (217, 119, 6),
        },
        _ => IconStyle {
            glyph: '\u{E7F4}',
            color: (37, 99, 235),
        },
    }
}

/// `glyph|#rrggbb` for each built-in icon, as the settings app's page reads them.
pub fn builtin_table() -> Vec<(String, String)> {
    BUILTIN
        .iter()
        .map(|name| {
            let style = builtin_style(name);
            let (r, g, b) = style.color;
            (
                (*name).to_string(),
                format!("{}|#{r:02x}{g:02x}{b:02x}", style.glyph),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_a_style_and_unknown_names_fall_back() {
        assert_eq!(builtin_style("nope"), builtin_style("monitor"));
        let table = builtin_table();
        assert_eq!(table.len(), BUILTIN.len());
        assert_eq!(
            table[1],
            ("couch".to_string(), "\u{E7F4}|#7c3aed".to_string())
        );
    }
}

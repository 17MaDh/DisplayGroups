//! Built-in icons, generated at run time: a rounded tile in the group's colour with a
//! Segoe Fluent Icons (or MDL2 Assets) glyph. No image files ship with the app, and the
//! same glyphs appear in the OSD.

use std::ffi::c_void;
use std::ptr::null_mut;

use crate::ffi_ui::{
    CreateFontW, DeleteObject, DrawTextW, SelectObject, SetBkMode, SetTextColor, RECT,
};
use crate::util::wide;

pub use dg_model::icons::{builtin_style, IconStyle, BUILTIN};

const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// Renders a multi-size `.ico` (32-bit BMP entries with alpha).
pub fn render_ico(style: IconStyle) -> Result<Vec<u8>, String> {
    let face = icon_font_face();
    let images = SIZES
        .iter()
        .map(|&s| render_bgra(style, s, &face).map(|px| bmp_entry(s, &px)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ico_container(&images))
}

/// Wraps an existing PNG in an `.ico` container (PNG entries work since Windows Vista).
pub fn png_to_ico(png: &[u8]) -> Result<Vec<u8>, String> {
    const SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if png.len() < 24 || &png[..8] != SIGNATURE || &png[12..16] != b"IHDR" {
        return Err("not a PNG file".into());
    }
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    if w == 0 || h == 0 || w > 256 || h > 256 {
        return Err(format!(
            "PNG icons must be at most 256×256 (this one is {w}×{h})"
        ));
    }
    Ok(ico_container(&[(w, png.to_vec())]))
}

/// Reads a custom `.ico` or `.png` for a group and returns the file name to store it
/// under in the icons folder (`custom-<name>.ico`) and its `.ico` bytes. The caller
/// writes the file and sets `group.icon` to `file:<file name>`.
pub fn import_file(source: &std::path::Path) -> Result<(String, Vec<u8>), String> {
    let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ico = match ext.as_str() {
        "ico" if bytes.len() > 6 && bytes[..4] == [0, 0, 1, 0] => bytes,
        "ico" => return Err("not an .ico file".into()),
        "png" => png_to_ico(&bytes)?,
        _ => return Err("the icon must be a .ico or .png file".into()),
    };
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = format!("custom-{}.ico", crate::integration::safe_file_name(&stem));
    Ok((name, ico))
}

/// The icon font available on this system: Segoe Fluent Icons (Windows 11) or
/// Segoe MDL2 Assets (Windows 10).
pub fn icon_font_face() -> String {
    let wanted = "Segoe Fluent Icons";
    let font = create_font(16, wanted);
    let mut buf = [0u16; 64];
    // SAFETY: memory DC used only to ask which face GDI actually picked.
    let actual = unsafe {
        let dc = CreateCompatibleDC(null_mut());
        let old = SelectObject(dc, font);
        let n = GetTextFaceW(dc, buf.len() as i32, buf.as_mut_ptr());
        SelectObject(dc, old);
        DeleteDC(dc);
        DeleteObject(font);
        String::from_utf16_lossy(&buf[..usize::try_from(n.max(1) - 1).unwrap_or(0)])
    };
    if actual.eq_ignore_ascii_case(wanted) {
        wanted.to_string()
    } else {
        "Segoe MDL2 Assets".to_string()
    }
}

/// Straight-alpha BGRA pixels, top row first.
fn render_bgra(style: IconStyle, size: u32, face: &str) -> Result<Vec<u8>, String> {
    let glyph = glyph_coverage(style.glyph, size, face)?;
    let s = size as f32;
    let radius = s * 0.22;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let tile = rounded_rect_coverage(x as f32, y as f32, s, radius);
            let g = f32::from(glyph[(y * size + x) as usize]) / 255.0;
            let mix = |c: u8| (f32::from(c) * (1.0 - g) + 255.0 * g).round() as u8;
            let i = ((y * size + x) * 4) as usize;
            out[i] = mix(style.color.2);
            out[i + 1] = mix(style.color.1);
            out[i + 2] = mix(style.color.0);
            out[i + 3] = (tile * 255.0).round() as u8;
        }
    }
    Ok(out)
}

/// Antialiased coverage (4×4 samples) of a rounded square of side `s`.
fn rounded_rect_coverage(px: f32, py: f32, s: f32, r: f32) -> f32 {
    let mut hits = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let x = px + (sx as f32 + 0.5) / 4.0;
            let y = py + (sy as f32 + 0.5) / 4.0;
            let cx = x.clamp(r, s - r);
            let cy = y.clamp(r, s - r);
            if (x - cx).powi(2) + (y - cy).powi(2) <= r * r {
                hits += 1;
            }
        }
    }
    hits as f32 / 16.0
}

/// Renders the glyph white-on-black with grayscale antialiasing and returns per-pixel
/// coverage (0..=255).
fn glyph_coverage(glyph: char, size: u32, face: &str) -> Result<Vec<u8>, String> {
    let side = i32::try_from(size).map_err(|e| e.to_string())?;
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: side,
        biHeight: -side, // top-down
        biPlanes: 1,
        biBitCount: 32,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        bmiColors: [0],
    };
    // SAFETY: DIB section owned by this function; every GDI object is released.
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        let mut bits: *mut c_void = null_mut();
        let bmp = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
        if bmp.is_null() || bits.is_null() {
            DeleteDC(dc);
            return Err("CreateDIBSection failed".into());
        }
        let old_bmp = SelectObject(dc, bmp);
        let font = create_font((size as f32 * 0.56).round() as i32, face);
        let old_font = SelectObject(dc, font);
        SetTextColor(dc, 0x00FF_FFFF);
        SetBkMode(dc, 1);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: side,
            bottom: side,
        };
        let text: Vec<u16> = glyph.encode_utf16(&mut [0; 2]).to_vec();
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut rect,
            0x1 | 0x4 | 0x20 | 0x800,
        );
        GdiFlush();
        let pixels = std::slice::from_raw_parts(bits as *const u8, (size * size * 4) as usize);
        let coverage = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0].max(p[1]).max(p[2]))
            .collect();
        SelectObject(dc, old_font);
        DeleteObject(font);
        SelectObject(dc, old_bmp);
        DeleteObject(bmp);
        DeleteDC(dc);
        Ok(coverage)
    }
}

fn create_font(height: i32, face: &str) -> *mut c_void {
    let f = wide(face);
    // SAFETY: logical font creation; callers delete it. 4 = ANTIALIASED_QUALITY.
    unsafe { CreateFontW(-height, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 4, 0, f.as_ptr()) }
}

/// One ICO image: BITMAPINFOHEADER (double height), bottom-up BGRA rows, empty AND mask.
fn bmp_entry(size: u32, bgra_top_down: &[u8]) -> (u32, Vec<u8>) {
    let row = (size * 4) as usize;
    let mask_row = size.div_ceil(32) as usize * 4;
    let mut data = Vec::with_capacity(40 + bgra_top_down.len() + mask_row * size as usize);
    let header = [
        40u32.to_le_bytes(),
        size.to_le_bytes(),
        (size * 2).to_le_bytes(),
    ];
    for h in header {
        data.extend_from_slice(&h);
    }
    data.extend_from_slice(&1u16.to_le_bytes()); // planes
    data.extend_from_slice(&32u16.to_le_bytes()); // bit count
    data.extend_from_slice(&[0u8; 24]); // compression .. clr important
    for y in (0..size as usize).rev() {
        data.extend_from_slice(&bgra_top_down[y * row..(y + 1) * row]);
    }
    data.resize(data.len() + mask_row * size as usize, 0);
    (size, data)
}

fn ico_container(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// An `HICON` destroyed on drop.
#[derive(Debug)]
pub struct OwnedIcon(*mut c_void);

impl OwnedIcon {
    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: created by CreateIconFromResourceEx and owned exclusively.
        unsafe { DestroyIcon(self.0) };
    }
}

/// Creates an icon of `size` pixels from `.ico` bytes (closest entry at or above the size).
pub fn hicon_from_ico(ico: &[u8], size: u32) -> Option<OwnedIcon> {
    if ico.len() < 6 {
        return None;
    }
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let entry = |i: usize| -> Option<(u32, usize, usize)> {
        let e = ico.get(6 + i * 16..6 + i * 16 + 16)?;
        let dim = if e[0] == 0 { 256 } else { u32::from(e[0]) };
        let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        Some((dim, off, len))
    };
    let (_, off, len) = (0..count)
        .filter_map(entry)
        .filter(|(d, _, _)| *d >= size)
        .min_by_key(|(d, _, _)| *d)
        .or_else(|| (0..count).filter_map(entry).max_by_key(|(d, _, _)| *d))?;
    let data = ico.get(off..off + len)?;
    let side = i32::try_from(size).ok()?;
    // SAFETY: `data` is a complete icon image; 0x0003_0000 is the required version.
    let h = unsafe {
        CreateIconFromResourceEx(data.as_ptr(), len as u32, 1, 0x0003_0000, side, side, 0)
    };
    (!h.is_null()).then_some(OwnedIcon(h))
}

#[allow(non_snake_case, clippy::upper_case_acronyms)]
#[repr(C)]
#[derive(Default)]
struct BITMAPINFOHEADER {
    biSize: u32,
    biWidth: i32,
    biHeight: i32,
    biPlanes: u16,
    biBitCount: u16,
    biCompression: u32,
    biSizeImage: u32,
    biXPelsPerMeter: i32,
    biYPelsPerMeter: i32,
    biClrUsed: u32,
    biClrImportant: u32,
}

#[allow(non_snake_case, clippy::upper_case_acronyms)]
#[repr(C)]
struct BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER,
    bmiColors: [u32; 1],
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn CreateDIBSection(
        dc: *mut c_void,
        info: *const BITMAPINFO,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> *mut c_void;
    fn GdiFlush() -> i32;
    fn GetTextFaceW(dc: *mut c_void, len: i32, name: *mut u16) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn CreateIconFromResourceEx(
        bits: *const u8,
        len: u32,
        icon: i32,
        version: u32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> *mut c_void;
    fn DestroyIcon(icon: *mut c_void) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_has_all_sizes_and_loads_as_hicon() {
        let ico = render_ico(builtin_style("couch")).unwrap();
        assert_eq!(u16::from_le_bytes([ico[4], ico[5]]) as usize, SIZES.len());
        for size in [16, 32] {
            let icon = hicon_from_ico(&ico, size).expect("HICON");
            assert!(!icon.as_raw().is_null());
        }
    }

    #[test]
    fn glyph_and_tile_are_drawn() {
        let face = icon_font_face();
        let px = render_bgra(builtin_style("monitor"), 64, &face).unwrap();
        let alpha = |x: usize, y: usize| px[(y * 64 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0, "rounded corner is transparent");
        assert_eq!(alpha(32, 32), 255, "centre is opaque");
        let white = px
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240)
            .count();
        assert!(white > 50, "the glyph is visible ({white} white pixels)");
    }

    #[test]
    fn png_is_wrapped_and_validated() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&64u32.to_be_bytes());
        let ico = png_to_ico(&png).unwrap();
        assert_eq!(ico[6], 64);
        assert!(png_to_ico(b"GIF89a....................").is_err());
    }

    #[test]
    fn custom_files_are_imported_as_ico() {
        let dir = std::env::temp_dir().join(format!("dg-icon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&32u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        let png_path = dir.join("Couch TV.png");
        std::fs::write(&png_path, &png).unwrap();
        let (name, ico) = import_file(&png_path).unwrap();
        assert_eq!(name, "custom-Couch TV.ico");
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);

        let ico_path = dir.join("tv.ico");
        std::fs::write(&ico_path, render_ico(builtin_style("couch")).unwrap()).unwrap();
        assert_eq!(import_file(&ico_path).unwrap().0, "custom-tv.ico");

        let fake = dir.join("fake.ico");
        std::fs::write(&fake, b"not an icon").unwrap();
        assert!(import_file(&fake).is_err());
        let gif = dir.join("x.gif");
        std::fs::write(&gif, b"GIF89a").unwrap();
        assert!(import_file(&gif).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

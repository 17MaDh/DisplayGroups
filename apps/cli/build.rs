//! Embeds into both executables the application manifest (common controls v6,
//! per-monitor DPI v2, UTF-8), the app icon and version information. Task Manager,
//! "Startup apps" and file properties show the description and product name, and
//! code signing services read them.

use std::path::Path;

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    let dir = Path::new(&dir);
    let manifest = dir.join("displaygroups.manifest");
    let icon = dir.join("../settings/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed=../../locales/en.json");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );

    let out = std::env::var("OUT_DIR").expect("set by cargo");
    for (bin, description) in [
        ("displaygroups", "DisplayGroups command line"),
        // The agent: this is the name people see in Task Manager and Startup apps.
        ("displaygroupsw", "DisplayGroups"),
    ] {
        let rc = Path::new(&out).join(format!("{bin}.rc"));
        std::fs::write(&rc, resource_script(bin, description, &icon)).expect("write .rc");
        embed_resource::compile_for(&rc, [bin], embed_resource::NONE)
            .manifest_optional()
            .expect("compile the version resource");
    }
}

fn resource_script(bin: &str, description: &str, icon: &Path) -> String {
    let env = |name: &str| std::env::var(name).expect("set by cargo");
    let (major, minor, patch) = (
        env("CARGO_PKG_VERSION_MAJOR"),
        env("CARGO_PKG_VERSION_MINOR"),
        env("CARGO_PKG_VERSION_PATCH"),
    );
    let version = env("CARGO_PKG_VERSION");
    let icon = icon.display().to_string().replace('\\', "\\\\");
    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "DisplayGroups contributors"
            VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "{bin}"
            VALUE "LegalCopyright", "Copyright (c) 2026 DisplayGroups contributors"
            VALUE "OriginalFilename", "{bin}.exe"
            VALUE "ProductName", "DisplayGroups"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}

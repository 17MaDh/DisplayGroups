# Contributing to DisplayGroups

Thanks for helping. This page covers building, testing, and the rules the project keeps.
[ARCHITECTURE.md](ARCHITECTURE.md) explains the design and why it is that way.

## Build and test

You need:
- Windows 10/11.
- [rustup](https://rustup.rs) (stable, MSVC; `rust-toolchain.toml` pins the channel).
- Visual Studio Build Tools with "Desktop development with C++".
- [Node.js](https://nodejs.org) LTS.

```powershell
cd apps\settings; npm ci; npm run build; cd ..\..   # the settings UI, embedded by Tauri
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

These are the checks CI runs ([`ci.yml`](.github/workflows/ci.yml)). A pull request needs all of
them to pass, plus `npm run typecheck` when you touch the UI.

- **Settings app with live reload:** `npm run tauri dev` in `apps\settings`.
- **Release build:** `.\scripts\build.ps1` builds the installer and ZIP
  ([docs/releasing.md](docs/releasing.md)).

## Where things live

| Path | What |
|---|---|
| `crates/dg-model` | Config schema, validation, migrations, atomic save. No Win32 |
| `crates/dg-identity` | EDID parsing and monitor matching. No Win32 |
| `crates/dg-engine` | The switching state machine, over the `DisplayBackend` trait. Tested against a fake backend |
| `crates/dg-hotkey` | Chord parsing and known conflicts |
| `crates/dg-ipc` | Pipe protocol and the secured pipe (for the lock-screen service) |
| `crates/dg-win` | **All** Win32 code: CCD, audio, tray, dialogs, OSD, shell integration |
| `apps/cli` | `displaygroups.exe` / `displaygroupsw.exe`: the CLI and the agent |
| `apps/settings` | The settings app: Tauri v2 (`src-tauri/`) and TypeScript/Vite (`ui/`) |
| `locales/` | UI strings shared by the native parts and the settings app |
| `scripts/` | `build.ps1` and `sign.ps1` |
| `tools/lockscreen-spike` | A feasibility test; never shipped |

## Rules

These rules protect users' machines. Reviews hold to them.

1. **Unsafe Win32 code stays in `dg-win`.** `dg-model`, `dg-identity` and `dg-engine` stay
   platform-free, so CI can test them without a display. `unsafe_op_in_unsafe_fn` is denied
   workspace-wide.
2. **Hotkeys use `RegisterHotKey` only.** Never use `WH_KEYBOARD_LL` hooks or Raw Input, and
   never anything that sees keystrokes on the secure desktop (ARCHITECTURE.md §8.4).
3. **Before/after actions run in the user's own session only**, from the CLI or agent. They are
   never sent to, stored for, or run by the LocalSystem service or the lock-screen broker.
4. **Per-user integration writes to HKCU only:** startup, the `displaygroups://` handler and
   shortcuts. Only the installer writes machine-wide.
5. **No network access and no telemetry.** The only download is the WebView2 bootstrapper, which
   the installer fetches if WebView2 is missing.
6. **Every switch can be undone.** Keep the confirm-or-revert countdown, the last-known-good
   layout and `recover` working. Never add a path that applies a layout without them, unless the
   user explicitly asked for `--no-confirm`.
7. **Every UI string goes in `locales/en.json`.** Use CSS logical properties (`inline-start`, not
   `left`) so right-to-left languages work.

## Testing on real displays

Switching displays can leave a screen black until the revert timer runs out.

- **On your own PC:**
  - `displaygroups validate <group>` asks Windows without changing anything.
  - `displaygroupsw agent --dry-run` shows what each hotkey would do.
- **Real `apply`, `toggle` and `recover`:** test on a VM or a second PC (ARCHITECTURE.md D3).
- **In a pull request:** say what you tested it on, including:
  - the GPU and driver;
  - the displays and how they are connected (HDMI/DP);
  - the Windows build;
  - which groups you switched between.
- **New engine behaviour:** add a test in `crates/dg-engine/src/tests.rs` with the fake backend.
  Use scripted `SetDisplayConfig` results, including the 31/87 errors where they matter.

## Translations

English (`en.json`) and Arabic (`ar.json`) ship today. To add a language:

1. Copy `locales/en.json` to `locales/<code>.json`, using the Windows language code without the
   region (`fr`, not `fr-FR`). Translate the values, and keep the keys and the `{placeholders}`.
2. Set `language.name` to the language's own name, for example `Français`. Settings lists it by
   that name.
3. Missing keys fall back to English one by one, so a partial translation still works.
   `cargo test` checks that every file:
   - is valid JSON;
   - uses only known keys;
   - keeps each placeholder.
   `ar.json` must also be complete.
4. **Right-to-left languages** (`ar`, `he`, `fa`, `ur`) switch the settings app, the tray menu,
   the on-screen display and the confirmation dialog to a right-to-left layout.
   - Wrap Latin technical terms that end in punctuation (such as `displaygroups://`) in U+2066…U+2069
     isolates, as `ar.json` does, so they are not reordered.
   - In UI code, pass resolutions, refresh rates and coordinates through `ltr()` (`ui/dom.ts`).
5. The installer and the portable ZIP ship every `locales/*.json` file. The default setting,
   "Windows language", picks the file that matches the Windows display language.

Debug builds read `locales/` from the repository, so a translation can be tried without copying
files: change the language in Settings, or set `"language": "<code>"` in the config.

## Releases and security

- **Releases:** maintainers follow [docs/releasing.md](docs/releasing.md).
- **Vulnerabilities:** report them privately, as [SECURITY.md](SECURITY.md) describes, not in a
  public issue.

By contributing you agree that your contributions are licensed under the [MIT license](LICENSE).

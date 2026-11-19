# DisplayGroups

Switch Windows between **groups of displays** with one click, a shortcut or a hotkey. For
example, "Desktop" (two monitors) and "Gaming on Couch" (the living-room TV only). Each group
sets exactly which displays are on and, for each one, the primary, resolution, refresh rate,
position, rotation, scale and HDR. It can also set the sound output and volume.

> **Status: pre-release.**
> - **Built and tested:**
>   - the switching engine and command line;
>   - the background agent (hotkeys, tray, on-screen display);
>   - the settings app;
>   - the installer and portable ZIP;
>   - the release pipeline.
> - **Next:** testing real switching on more hardware before 1.0.
> - **Waiting on a feasibility test:** hotkeys on the lock screen
>   ([docs/lockscreen-spike.md](docs/lockscreen-spike.md)).
>
> See [ARCHITECTURE.md](ARCHITECTURE.md) for the design.

## Features

- **Groups** remember monitors by their EDID identity, so they survive cable and port changes.
  Twin monitors without serial numbers are told apart by connector.
- **Switching**
  - Tries the most exact method first and falls back to safer ones.
  - Asks "Keep these display settings?" and reverts after 15 seconds unless you confirm.
  - `recover` restores the last layout that worked.
- **Hotkeys**
  - Global chords such as `Ctrl+Alt+F12`.
  - When another program holds a chord, the agent says which one, if it can tell.
- **Tray and on-screen display:** a quick-switch menu, and a short "Gaming on Couch" toast after
  each switch.
- **Settings app**
  - A drag-and-drop layout editor, with resolution and refresh pickers from the modes each
    monitor really offers.
  - Pages for hotkeys, shortcuts, theme (System/Dark/Light), import/export and diagnostics.
- **Shortcuts:** desktop or Start menu shortcuts that switch to a group, with built-in or custom
  icons.
- **Links:** `displaygroups://` links for Stream Deck and automation tools (opt-in).
- **Disconnect fallback:** optionally switches to a fallback group when a display disappears.
- **Before/after actions** (off by default): run a program or command as you, never as a
  system service.
- **Private:** no network access, no telemetry, no account.

## Install

Download from [Releases](../../releases):

- **Installer** (`DisplayGroups-<version>-setup-x64.exe`)
  - Installs for all users in `C:\Program Files\DisplayGroups` and needs admin rights.
  - Asks whether to start with Windows and whether to enable `displaygroups://` links. You can
    change both later in Settings.
  - Uninstall from Settings → Apps. Tick "Delete the application data" to remove your groups and
    logs too.
- **Portable** (`DisplayGroups-<version>-portable-x64.zip`)
  - Unzip anywhere you can write to and run `displaygroups-settings.exe`.
  - Everything is kept in a `data` folder next to it.
  - Delete `portable.flag` to use the normal locations instead.

The settings app uses Microsoft Edge WebView2, which Windows 11 includes. On Windows 10 the
installer downloads it if it is missing.

**Check a download** against `SHA256SUMS.txt` from the same release:

```powershell
Get-FileHash .\DisplayGroups-1.0.0-setup-x64.exe -Algorithm SHA256
```

### Smart App Control and SmartScreen

Windows checks programs before running them:

- **Smart App Control** (Windows 11) blocks programs that are not code-signed. You cannot make an
  exception for one app.
  - Signed releases of DisplayGroups run normally.
  - Unsigned ones are blocked: a release says in its notes whether it is signed, and so are
    builds you make yourself.
  - We don't recommend turning Smart App Control off just for this app. Wait for a signed release,
    or use a machine where it is already off.
- **SmartScreen** may show "Windows protected your PC" for a new or unsigned download, until the
  download builds a reputation.
  - Check the SHA-256 first.
  - Then use *More info → Run anyway*.

Releases are signed through the [SignPath Foundation](https://signpath.org) programme once the
project is accepted ([docs/releasing.md](docs/releasing.md)). A self-signed certificate does not
satisfy Smart App Control, even when you trust it on your own PC.

## Getting started

1. Arrange your displays the way you want with Windows Settings → Display. Then, in
   DisplayGroups, choose **Capture current setup** and name the group.
2. Repeat for each layout you use.
3. Add a hotkey (Hotkeys page), a shortcut (Shortcuts page), or use the tray menu.

## Command line

`displaygroups.exe` is the console version and `displaygroupsw.exe` the same program without a
console window (use it in shortcuts). Groups can be named or given by id.

| Command | What it does |
|---|---|
| `displaygroups capture "Desktop"` | Saves the current layout as a group (creates or overwrites it) |
| `displaygroups apply "Gaming on Couch"` | Switches to a group and asks "Keep these display settings?" (reverts after 15 s) |
| `displaygroups toggle "Desktop" "Gaming on Couch"` | Switches to whichever of these is not on screen now |
| `displaygroups validate "Desktop"` | Asks Windows whether the group can be applied, **without changing anything** |
| `displaygroups list` / `status` | Shows groups and monitors / what is on screen |
| `displaygroups duplicate "Desktop"` | Copies a group |
| `displaygroups recover [--extend]` | Restores the last layout that worked (or extends all displays) |
| `displaygroups shortcut "Gaming on Couch" [--desktop] [--start-menu] [--icon builtin:couch]` | Creates a shortcut that switches to the group. Built-in icons: `monitor`, `couch`, `dual`, `gaming`, `work`; or a `.ico`/`.png` file |
| `displaygroups startup on\|off\|status` | Starts the agent when you sign in (current user only) |
| `displaygroups protocol on\|off\|status` | Handles `displaygroups://apply/<id>` and `displaygroups://toggle/<id>,<id>` links (current user only) |
| `displaygroupsw agent [--dry-run]` | The background agent: hotkeys, tray, on-screen display, config reload. `--dry-run` only shows what a hotkey would do |

Options:
- `--no-confirm` (alias `--force`) skips the countdown.
- `--json` prints machine-readable output.
- `--config <path>` uses another config file.

Hotkeys live in `config.json`, and the settings app edits them. Example:

```json
"hotkeys": [
  { "chord": "Ctrl+Alt+F12", "action": { "type": "toggle", "groups": ["<desktop id>", "<couch id>"] } }
]
```

**Exit codes:**

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | error |
| 2 | usage |
| 3 | not kept (reverted) |
| 4 | monitor missing |
| 5 | Windows is locked |
| 6 | Windows rejected the group |

**Files:**

| What | Where |
|---|---|
| Config | `%APPDATA%\DisplayGroups\config.json` |
| Logs and the last layout that worked | `%LOCALAPPDATA%\DisplayGroups\` |
| Portable | `.\data` next to the executables, while `portable.flag` exists |

## Troubleshooting

| Problem | Fix |
|---|---|
| The screen went black or wrong after a switch | Wait 15 seconds; it reverts on its own unless you confirm. Or press your hotkey again, or run `displaygroups recover` (`recover --extend` turns every display on). |
| "Monitor missing" | The display is off, unplugged or asleep. TVs often drop off HDMI when in standby. Turn it on, or remove it from the group. |
| "Windows rejected the group" | Run `displaygroups validate "<group>"` for details. Re-capturing the group usually fixes it after a driver update. |
| A hotkey does nothing | The Hotkeys page shows whether the chord is free and, if not, which program holds it. Check that the agent (tray icon) is running. |
| Scale or refresh rate is not what you set | Check the group in the settings app. Some TVs only offer 120 Hz on certain HDMI ports or in game mode. |
| Something else | Diagnostics → Open logs folder. Attach the latest log to a bug report. |

## Building from source

Requirements:
- Windows 10/11.
- [rustup](https://rustup.rs) (stable, MSVC).
- Visual Studio Build Tools with "Desktop development with C++".
- [Node.js](https://nodejs.org) LTS.

```powershell
.\scripts\build.ps1                  # UI, tests, release build, installer and ZIP in dist-release\
```

Or step by step:

```powershell
cd apps\settings; npm ci; npm run build; cd ..\..
cargo test --workspace
cargo build --release --features displaygroups-settings/custom-protocol
```

`npm run tauri dev` in `apps\settings` runs the settings app with live reload. Builds you make
yourself are unsigned (see [Smart App Control](#smart-app-control-and-smartscreen)): they may run
from a terminal and still be blocked from Explorer.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the project rules and how to test, and
[SECURITY.md](SECURITY.md) to report a vulnerability.

## License

[MIT](LICENSE)

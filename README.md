# DisplayGroups

Switch Windows between **groups of displays** with one click, shortcut or hotkey: for example
"Desktop" (two monitors) and "Gaming on Couch" (the living-room TV only). Each group sets
exactly which displays are on, the primary, resolution, refresh rate, position, rotation and
scale, and optionally the audio output and volume.

> **Status: pre-alpha.**
> - **Done:**
>   - the switching engine and command line (Phase 2);
>   - the background agent with hotkeys and an on-screen display (Phase 3);
>   - the tray menu, the "Keep these display settings?" countdown, and shortcut and startup
>     integration (Phase 4a);
>   - the settings app with the drag-and-drop layout editor (Phase 4b).
> - **Waiting on a feasibility test:** lock-screen support (see
>   [docs/lockscreen-spike.md](docs/lockscreen-spike.md)).
> - **Next:** the tray, the settings app and the installer.
>
> See [ARCHITECTURE.md](ARCHITECTURE.md).

## Command line

| Command | What it does |
|---|---|
| `displaygroups capture "Desktop"` | Saves the current layout as a group (creates or overwrites it) |
| `displaygroups apply "Gaming on Couch"` | Switches to a group and asks "Keep these display settings?" (reverts after 15 s) |
| `displaygroups toggle "Desktop" "Gaming on Couch"` | Switches to whichever of these is not on screen now |
| `displaygroups validate "Desktop"` | Asks Windows whether the group can be applied, **without changing anything** |
| `displaygroups list` / `status` | Shows groups and monitors / what is on screen |
| `displaygroups duplicate "Desktop"` | Copies a group |
| `displaygroups recover [--extend]` | Restores the last layout that worked (or extends all displays) |
| `displaygroups shortcut "Gaming on Couch" [--desktop] [--start-menu] [--icon builtin:couch]` | Creates a shortcut that switches to the group. Built-in icons: `monitor`, `couch`, `dual`, `gaming`, `work`; or give a `.ico`/`.png` file |
| `displaygroups startup on\|off\|status` | Starts the agent when you sign in (current user only) |
| `displaygroups protocol on\|off\|status` | Handles `displaygroups://apply/<id>` and `displaygroups://toggle/<id>,<id>` links (current user only) |
| `displaygroupsw agent [--dry-run]` | Runs in the background: registers the hotkeys in `config.json`, shows the on-screen display after a switch, and reloads the config when it changes. `--dry-run` only shows what a hotkey would do |

Hotkeys are entries in `config.json`. Example:

```json
"hotkeys": [
  { "chord": "Ctrl+Alt+F12", "action": { "type": "toggle", "groups": ["<desktop id>", "<couch id>"] } }
]
```

If another program already holds a chord, the agent says which one, when it can tell.

- `--no-confirm` (alias `--force`) skips the countdown. `--json` prints machine-readable output.
- `--config <path>` uses another config file.
- `displaygroupsw.exe` takes the same commands without opening a console window. Use it for shortcuts.

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
- Config: `%APPDATA%\DisplayGroups\config.json`.
- Logs and last-known-good layout: `%LOCALAPPDATA%\DisplayGroups\`.
- A `portable.flag` file next to the executables keeps everything in `.\data` instead.

## Building from source

Requirements:
- Windows 10/11.
- [rustup](https://rustup.rs) (stable, MSVC).
- Visual Studio Build Tools with the "Desktop development with C++" workload.

The settings app also needs [Node.js](https://nodejs.org) LTS.

```powershell
cd apps\settings; npm ci; npm run build; cd ..\..   # the UI that gets embedded
cargo test --workspace
cargo build --release --features displaygroups-settings/custom-protocol
```

The binaries in `target\release` are:
- `displaygroups.exe` and `displaygroupsw.exe`: the CLI, with and without a console window.
- `displaygroups-settings.exe`: the settings app.

To work on the UI with live reload, run `npm run tauri dev` in `apps\settings`.

### Smart App Control

Windows 11 **Smart App Control** blocks unsigned programs, and locally built binaries are
unsigned. On a PC where SAC is on:
- builds may run from a terminal but be blocked when started from Explorer or a shortcut;
- release builds will be code-signed (planned: SignPath Foundation);
- until then, test on a machine where SAC is off.

## License

[MIT](LICENSE)

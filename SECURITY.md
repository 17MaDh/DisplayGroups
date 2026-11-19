# Security policy

## Reporting a vulnerability

Report it privately through GitHub: open the repository's **Security** tab and choose
**Report a vulnerability**. Please do not open a public issue.

Include what you found, the version, and steps to reproduce. Once a fix is released, you will be
credited in the release notes unless you prefer not to be.

## Supported versions

Only the latest release gets security fixes.

## What DisplayGroups does, and does not do, on your PC

This is here so you can judge the risk, and so reports can say which property they break.

- **Privileges**
  - The CLI, the agent and the settings app run as you, without elevation.
  - The installer needs admin rights to install to Program Files.
  - Startup, `displaygroups://` and shortcuts are written to your user registry (HKCU) only.
- **No network:** DisplayGroups makes no network connections and collects no telemetry. The
  installer downloads the Microsoft WebView2 bootstrapper only if WebView2 is missing.
- **Keyboard:** hotkeys use `RegisterHotKey`, which receives only the registered chords.
  DisplayGroups never installs keyboard hooks.
- **Automatic switching** (only while a trigger is set up):
  - The agent reads the names of running programs every 2 s, and window titles when a trigger
    filters by title.
  - It checks for game controllers after device changes.
  - All of this stays in memory and is compared with your triggers. Nothing is stored or sent,
    and only matches are logged.
  - It never injects into or hooks other processes.
- **Before/after actions**
  - Off by default.
  - Run as you, from the CLI or agent only.
  - Never run from `displaygroups://` links unless the group allows it.
- **Lock-screen support** (optional; not in the installer yet)
  - A LocalSystem service runs a broker on the secure desktop. The broker may only apply display
    geometry from a profile you synced.
  - Its pipe:
    - accepts local clients only;
    - checks the caller's session and SID;
    - limits messages to 64 KB.
  - The service never receives or runs actions. ARCHITECTURE.md §8 has the details.
- **Downloads:** every release lists SHA-256 checksums. Signed releases are signed through the
  SignPath Foundation programme ([docs/releasing.md](docs/releasing.md)).

## In scope

- Privilege escalation through the service, the broker or its pipe.
- Code execution through `displaygroups://` links, config import or shortcut arguments.
- Anything that makes DisplayGroups read keystrokes beyond its own hotkeys.
- Problems in the release and signing pipeline.

Display configurations that Windows itself rejects or reverts are bugs, not vulnerabilities.
Please report them as normal issues.

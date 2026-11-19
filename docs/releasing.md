# Releasing DisplayGroups

A release is built by [`.github/workflows/release.yml`](../.github/workflows/release.yml) when a
`v*` tag is pushed. It produces a **draft** GitHub release with three files:

| File | What it is |
|---|---|
| `DisplayGroups-<version>-setup-x64.exe` | Per-machine NSIS installer (Program Files, needs admin) |
| `DisplayGroups-<version>-portable-x64.zip` | The three executables, `locales\`, `portable.flag`, README and license |
| `SHA256SUMS.txt` | SHA-256 of both, in `sha256sum` format |

The same files come out of a local build:

```powershell
.\scripts\build.ps1                    # everything, unsigned, into dist-release\
.\scripts\build.ps1 -Version 1.2.0     # also checks that every manifest says 1.2.0
```

`build.ps1` never runs the installer. Installing changes the system, so it is tested on a VM
(see the checklist below).

## 1. Cut a release

1. Set the new version in all three places. The build stops if they differ:
   - `Cargo.toml`, `[workspace.package] version`
   - `apps/settings/src-tauri/tauri.conf.json`, `version`
   - `apps/settings/package.json`, via `npm version <x.y.z> --no-git-tag-version` in
     `apps\settings` (this also updates `package-lock.json`)
2. Run `cargo build` once, so `Cargo.lock` records the new version. Release builds use
   `--locked` and fail on a stale lock file.
3. Build locally with `.\scripts\build.ps1 -Version <x.y.z>`, then run the
   [VM checklist](#3-installer-checklist-vm) against `dist-release\`.
4. Commit, then tag and push:

   ```powershell
   git tag v<x.y.z>
   git push origin v<x.y.z>
   ```

5. Wait for the workflow, then check the draft: its files, the signing line in the notes, and
   the generated changelog. Publish it by hand.

## 2. Code signing

Unsigned builds work, but SmartScreen warns about them and **Smart App Control blocks them**.
Smart App Control also blocks the NSIS plugin DLLs and the uninstaller, so a release has to
sign every PE file it contains, not only the installer.

### Modes (`scripts/sign.ps1`)

Tauri's bundler calls `scripts/sign.ps1 <file>` for each file it would sign:
- the three executables;
- the NSIS plugin DLLs (`System.dll`, `nsDialogs.dll`, `StartMenu.dll`, `NSISdl.dll`,
  `nsis_tauri_utils.dll`);
- the uninstaller, while the installer is being compiled;
- the installer.

`build.ps1` also calls it for the executables in the ZIP. `DG_SIGN_MODE` picks the method:

| `DG_SIGN_MODE` | Use |
|---|---|
| unset | Nothing is signed (default, and the CI behaviour while SignPath is not set up) |
| `signtool` | Local signing with a certificate in the Windows store: `SIGN_CERT_THUMBPRINT`, optional `SIGN_TIMESTAMP_URL` |
| `signpath` | The CI flow below |

A self-signed certificate does not satisfy Smart App Control, even when it is trusted on the PC.

### SignPath (CI)

SignPath's open-source programme accepts only builds whose origin it can verify. That means:
- a GitHub-hosted runner;
- files uploaded as a workflow artifact;
- a signing request made by
  [`signpath/github-action-submit-signing-request`](https://github.com/signpath/github-action-submit-signing-request).

A signing request is a workflow step, not a command the bundler can call. The workflow therefore
packages the release in rounds. In each round, `sign.ps1` does one of two things with each file:
- if a signed copy of the *identical* bytes exists under `%DG_SIGN_DIR%\signed`, it swaps it in;
- otherwise it queues the file for the next request, named `<sha256>.exe` or `<sha256>.dll`.

| Round | Queued and signed |
|---|---|
| 1 | The executables, the plugin DLLs and an uninstaller |
| 2 | The uninstaller again. It embeds the plugin DLLs, which are now signed, so its bytes changed |
| 3 | Nothing may be queued. The installer is then signed on its own |

This depends on the uninstaller being byte-for-byte reproducible from the same inputs. It is:
two local builds gave the same SHA-256. If round 3 still finds unsigned files, the workflow stops
rather than ship a partly signed release. A last step unpacks the installer (7-Zip) and the ZIP,
and fails unless every `.exe` and `.dll` has a valid signature.

**Repository settings** (Settings → Secrets and variables → Actions):

| Name | Kind | Value |
|---|---|---|
| `SIGNPATH_ORGANIZATION_ID` | variable | Organization ID. Setting it turns signing on |
| `SIGNPATH_PROJECT_SLUG` | variable | Project slug |
| `SIGNPATH_SIGNING_POLICY_SLUG` | variable | `release-signing` or `test-signing` |
| `SIGNPATH_FILES_CONFIGURATION` | variable | Artifact configuration for rounds 1–2 |
| `SIGNPATH_INSTALLER_CONFIGURATION` | variable | Artifact configuration for the installer |
| `SIGNPATH_API_TOKEN` | secret | Token of a SignPath user with submitter rights |

**Artifact configurations.** The uploaded artifact is a ZIP, so the root element is `<zip-file>`.

Rounds 1–2, every file in the artifact:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file-set>
      <include path="*.exe" min-matches="0" max-matches="unbounded" />
      <include path="*.dll" min-matches="0" max-matches="unbounded" />
      <for-each>
        <authenticode-sign />
      </for-each>
    </pe-file-set>
  </zip-file>
</artifact-configuration>
```

The installer:

```xml
<?xml version="1.0" encoding="utf-8"?>
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <zip-file>
    <pe-file-set>
      <include path="DisplayGroups-*-setup-x64.exe" />
      <for-each>
        <authenticode-sign />
      </for-each>
    </pe-file-set>
  </zip-file>
</artifact-configuration>
```

**To settle with SignPath during onboarding:**
- **NSIS plugin DLLs.** These are third-party binaries, from NSIS and from Tauri's
  `nsis_tauri_utils`. Confirm that the programme allows them to be signed with the project's
  certificate. If it does not, narrow the first configuration.
- **Manual approval.** If the policy needs approval for each request, a release waits three
  times: rounds 1, 2 and the installer.
- **File metadata.** The executables carry product name, description and version
  (`apps/cli/build.rs` and Tauri), which SignPath can restrict on.
- **Attribution.** SignPath Foundation asks for "Free code signing provided by SignPath.io,
  certificate by SignPath Foundation" on the project page. The release notes already carry it.

## 3. Installer checklist (VM)

Run this on the Windows 11 VM or secondary PC (ARCHITECTURE.md D3), never on a PC you depend on.

**Install**

- [ ] `DisplayGroups-<v>-setup-x64.exe` matches `SHA256SUMS.txt` (`Get-FileHash`).
- [ ] UAC prompt, then the license page and the install folder (`C:\Program Files\DisplayGroups`).
- [ ] "Start … when you sign in?" appears; Yes is the default.
- [ ] The `displaygroups://` question appears; No is the default.
- [ ] Finish → "Run DisplayGroups" opens the settings app, not elevated (check in Task Manager),
      and the tray icon appears (agent started).
- [ ] The folder holds `displaygroups.exe`, `displaygroupsw.exe`, `displaygroups-settings.exe`,
      `locales\en.json` and `uninstall.exe`. File properties show the version.
- [ ] Settings → Apps lists DisplayGroups with its version and publisher.
- [ ] The Start menu shortcut opens the settings app.
- [ ] `displaygroups startup status` and `displaygroups protocol status` match the answers.

**Upgrade** (install the same or a newer build over it)

- [ ] The agent and settings app are closed first.
- [ ] Groups, settings and startup are kept, and nothing is asked again when already on.

**Uninstall** (Settings → Apps)

- [ ] Without "Delete the application data": the program folder is gone.
- [ ] The HKCU `Run` value and `HKCU\Software\Classes\displaygroups` are gone.
- [ ] `Start Menu\Programs\DisplayGroups` (shortcuts made in the app) is gone. Desktop shortcuts
      the user made stay.
- [ ] `%APPDATA%\DisplayGroups` is kept.
- [ ] With the checkbox: `%APPDATA%\DisplayGroups` and `%LOCALAPPDATA%\DisplayGroups` are deleted too.

**Silent**

- [ ] `setup.exe /S` installs with no questions.
- [ ] `"C:\Program Files\DisplayGroups\uninstall.exe" /S` removes it.

**Portable ZIP**

- [ ] Unzip to a folder you can write to and run `displaygroups-settings.exe`.
- [ ] Config and logs are created in `.\data`.
- [ ] Nothing is written to `%APPDATA%\DisplayGroups`.

**Smart App Control on** (signed builds only)

- [ ] Install, run, switch and uninstall all work with SAC in enforcement mode.

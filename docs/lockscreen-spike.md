# Lock-screen spike (ARCHITECTURE.md §8.5)

This is the go/no-go test that decides whether lock-screen hotkeys can ship. It installs
a **temporary LocalSystem service**, so run it **only on a test VM or a secondary PC**
(decision D3). It never changes the displays: the display call is `SDC_VALIDATE` only.

## Build

```powershell
cargo build --release -p lockscreen-spike
```

Copy `target\release\lockscreen-spike.exe` to the test machine, e.g. `C:\spike\`. If Smart
App Control is on there, the unsigned binary will be blocked; use a machine where it is off.

## Run (elevated PowerShell on the test machine)

```powershell
C:\spike\lockscreen-spike.exe install
```

1. Lock the PC with **Win+L**. Wait about 3 seconds.
2. Press **Ctrl+Alt+F11** two or three times on the lock screen.
3. Unlock.

Then:

```powershell
C:\spike\lockscreen-spike.exe report
C:\spike\lockscreen-spike.exe uninstall
```

`report` prints the log (`%ProgramData%\DisplayGroupsSpike\spike.log`) and a three-line
verdict:

| Question | Go when the log says |
|---|---|
| 1. Broker runs on the Winlogon desktop while locked | `thread desktop=Winlogon` |
| 2. `RegisterHotKey` delivers the chord there | `HOTKEY RECEIVED` |
| 3. `SetDisplayConfig` is allowed from there | `SetDisplayConfig VALIDATE -> 0` |

Send the whole `report` output back. If question 2 is "no", lock-screen hotkeys ship
disabled. Per §8.4 the design will **not** fall back to keyboard hooks or raw input.

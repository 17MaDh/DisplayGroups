# Linux test checklist (Ubuntu GNOME)

The automated tests run against GNOME's display service without a real desktop
(ARCHITECTURE.md §16.4–16.7). This list covers what they cannot: GNOME Shell's own
dialogs and top bar, real monitors, and the packages on a real system. Tick each item,
and note what happened for anything that did not work.

## Before you start

- [ ] Ubuntu 24.04 or later, desktop edition, fully updated. Note the version:
      `lsb_release -d`.
- [ ] Note the session: `echo $XDG_SESSION_TYPE` (`wayland` by default; "Ubuntu on Xorg"
      at the login screen gives `x11`). Run the list on Wayland first.
- [ ] Two displays at least. Real hardware is best, for example a monitor and a TV over
      HDMI. In VirtualBox: Settings → Display → Monitor Count 2, then View → Virtual
      Screen 2 → Enable. Virtual displays have no EDID, so items about EDIDs need real
      ones.
- [ ] Download the `.deb`, the AppImage and `SHA256SUMS.txt` from the same release, and
      check them: `sha256sum -c SHA256SUMS.txt --ignore-missing`.

## 1. Install the .deb

- [ ] `sudo apt install ./DisplayGroups-<version>-linux-x64.deb` finishes without errors.
- [ ] DisplayGroups appears in the app grid, with its icon.
- [ ] In a terminal, `displaygroups --version` and `displaygroups status` work.

## 2. The settings app

- [ ] It opens from the app grid. Light and dark follow Settings → Appearance.
- [ ] "On screen now" lists your displays with the right names (from the EDID on real
      monitors) and modes.
- [ ] Change something in GNOME Settings → Displays: the window follows within a second.
- [ ] **Capture current setup** makes a group. **New group** builds another, including a
      display that is off right now.
- [ ] The layout editor offers the resolutions and refresh rates each display really has.
- [ ] **Identify** shows each display's number and name on that display for 3 seconds.
- [ ] Icons: **Choose file…** opens GNOME's file chooser; a PNG you pick shows as the
      group's icon.
- [ ] Settings → Language → العربية: the window turns right-to-left. Switch back.

## 3. Switching

- [ ] **Test** a group: the displays change and GNOME asks "Keep these display settings?".
  - [ ] **Keep Changes**: the layout stays, and the window shows that group on screen.
  - [ ] **Revert**, or no answer for 20 seconds: the old layout comes back.
- [ ] Scale (125%, 150%), the primary display and positions come out as set in the group.
- [ ] A rotated (portrait) display rotates the right way. *Not checked yet on hardware.*
- [ ] A TV at 120 Hz gets 120 Hz (GNOME may list it as 119.88).
- [ ] `displaygroups recover --extend` turns every display back on.
- [ ] On HiDPI displays, the settings window and Identify labels are sharp and the right
      size.

## 4. The agent, tray and notifications

- [ ] Opening the settings app starts the agent: the DisplayGroups icon is in the top bar.
- [ ] Its menu lists the groups, with the one on screen ticked. Choosing another switches.
- [ ] A notification with the group's name and mode appears after each switch.
- [ ] Settings → **Start when I log in** on, log out and in: the tray icon is back.

## 5. Hotkeys

- [ ] Add a hotkey, such as Super+Shift+F12 (on Wayland, Ctrl+Alt+F1…F12 belong to the text
      consoles). It appears in GNOME Settings → Keyboard → View and Customize Shortcuts →
      Custom Shortcuts as "DisplayGroups: …".
- [ ] Pressing it switches, also when the agent is not running (quit it from the tray).
- [ ] Your own custom shortcuts are still there and unchanged.

## 6. Automatic switching

- [ ] A program trigger (for example `gnome-calculator`): starting it switches to the group;
      closing it switches back after the delay.
- [ ] A game controller trigger, with a USB or Bluetooth gamepad.
- [ ] Disconnect fallback: with it on, unplugging the only display that is on switches to
      the fallback group.

## 7. Links and launchers

- [ ] Settings → links on, then `xdg-open "displaygroups://apply/<group id>"` switches
      (`displaygroups list` shows each group's id).
- [ ] Shortcuts page → **Desktop** and **App grid**: each launcher switches to its group.

## 8. The AppImage

Remove the `.deb` first (`sudo apt remove display-groups`), so only the AppImage is in use.

- [ ] `sudo apt install libfuse2t64`, then `chmod +x` the AppImage and run it: the settings
      app opens and the tray icon appears.
- [ ] Close the settings window: the tray icon stays.
- [ ] Start at login, a hotkey, a link and a launcher each run the AppImage file (look at
      `~/.config/autostart/displaygroups.desktop` and the custom shortcut's command).
- [ ] `./DisplayGroups-<version>-linux-x64.AppImage status` prints the status.

## 9. An X11 session

Log out, choose "Ubuntu on Xorg" (the gear at the login screen), and repeat:

- [ ] section 2's "On screen now" and **Test** from section 3;
- [ ] the tray and a hotkey;
- [ ] **Identify**.

## 10. Remove

- [ ] Before removing: Settings → Start when I log in off, links off, and delete the
      launchers you made. These live in your home folder, and removing the package cannot
      reach them.
- [ ] `sudo apt remove display-groups` removes the app. Your groups stay in
      `~/.config/displaygroups`; delete that folder too for a clean slate.

## What to report

Open an issue with the bug form, and include:

- the Ubuntu version and the session type (Wayland or X11);
- the graphics card and driver, and each display and how it is connected;
- the items that failed, and what happened instead;
- the log: `~/.local/state/displaygroups/logs`.

// Hotkeys: record a chord, check it live against other programs (Windows) or GNOME's
// own shortcuts (Linux, where each hotkey becomes a GNOME custom shortcut), pick the
// action.

import { api, type Hotkey, type HotkeyAction, type HotkeyProbe } from "../api";
import { h, icon, select, toggle } from "../dom";
import { t } from "../i18n";
import { changed, onLinux, state } from "../state";

const probes = new Map<string, HotkeyProbe | "pending">();
let recording: Hotkey | null = null;

/** Maps KeyboardEvent.code to the key names dg-hotkey parses. */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (/^Numpad[0-9]$/.test(code)) return code;
  const named: Record<string, string> = {
    ArrowLeft: "Left",
    ArrowRight: "Right",
    ArrowUp: "Up",
    ArrowDown: "Down",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    Insert: "Insert",
    Delete: "Delete",
    Space: "Space",
    Tab: "Tab",
    Enter: "Enter",
    Backspace: "Backspace",
    Pause: "Pause",
    PrintScreen: "PrintScreen",
    ScrollLock: "ScrollLock",
    Minus: "Minus",
    Equal: "Equals",
    Comma: "Comma",
    Period: "Period",
    Slash: "Slash",
    Semicolon: "Semicolon",
    Quote: "Quote",
    Backquote: "Backquote",
    BracketLeft: "LeftBracket",
    BracketRight: "RightBracket",
    Backslash: "Backslash",
  };
  return named[code] ?? null;
}

function onRecordKey(e: KeyboardEvent): void {
  if (!recording) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.code === "Escape") {
    stopRecording();
    return;
  }
  const key = keyName(e.code);
  if (!key) return; // a modifier on its own: keep waiting
  const parts = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && (onLinux() ? "Super" : "Win"), key].filter(Boolean);
  recording.chord = parts.join("+");
  stopRecording();
}

function stopRecording(): void {
  recording = null;
  window.removeEventListener("keydown", onRecordKey, true);
  changed();
}

function status(chord: string): HTMLElement {
  const cached = probes.get(chord);
  if (!cached) {
    probes.set(chord, "pending");
    api
      .probeHotkey(chord)
      .then((p) => probes.set(chord, p))
      .catch(() => probes.delete(chord))
      .finally(() => changed());
  }
  if (!cached || cached === "pending") return h("span", { class: "badge" }, "…");
  if (!cached.valid) return h("span", { class: "badge bad" }, cached.error ?? "?");
  if (cached.reserved) return h("span", { class: "badge bad" }, t("hotkeys.reserved", { what: cached.reserved }));
  // Linux: the system says whether it is one of our GNOME shortcuts already. A clash
  // with GNOME's own shortcuts still wins (GNOME runs its own one).
  if (cached.ours && cached.free) return h("span", { class: "badge ok" }, t("hotkeys.ours"));
  if (cached.free) {
    return cached.known.length
      ? h("span", { class: "badge warn", title: cached.known.join(", ") }, t("hotkeys.takenBy", { owner: cached.known.join(", ") }))
      : h("span", { class: "badge ok" }, t("hotkeys.free"));
  }
  // Held by someone. It is ours only if the agent runs and the chord is already saved.
  const saved = (JSON.parse(state.saved) as { hotkeys?: { chord: string }[] }).hotkeys ?? [];
  const ours = cached.ours === null && cached.agentRunning && saved.some((s) => s.chord.replace(/\s/g, "").toLowerCase() === (cached.canonical ?? "").toLowerCase());
  const hint = cached.known.length ? ` (${t("hotkeys.takenBy", { owner: cached.known.join(", ") })})` : "";
  return h("span", { class: `badge ${ours ? "ok" : "bad"}` }, ours ? t("hotkeys.ours") : t("hotkeys.taken") + hint);
}

function groupPicker(action: HotkeyAction): HTMLElement | null {
  const groups = state.config.groups;
  if (action.type === "recover") return null;
  if (action.type === "apply") {
    return select(
      groups.map((g) => [g.id, g.name] as [string, string]),
      action.group,
      (v) => ((action.group = v), changed()),
    );
  }
  return h(
    "span",
    { class: "row" },
    ...groups.map((g) =>
      h(
        "label",
        { class: "row" },
        h("input", {
          type: "checkbox",
          checked: action.groups.includes(g.id),
          onchange: (e: Event) => {
            const on = (e.target as HTMLInputElement).checked;
            action.groups = on ? [...action.groups, g.id] : action.groups.filter((id) => id !== g.id);
            changed();
          },
        }),
        g.name,
      ),
    ),
  );
}

function changeType(hk: Hotkey, type: HotkeyAction["type"]): void {
  const ids = state.config.groups.map((g) => g.id);
  const current = "groups" in hk.action ? hk.action.groups : "group" in hk.action ? [hk.action.group] : [];
  const pool = current.length ? current : ids;
  hk.action =
    type === "recover"
      ? { type }
      : type === "apply"
        ? { type, group: pool[0] ?? ids[0] }
        : { type, groups: pool.length >= 2 ? pool : ids.slice(0, 2) };
  changed();
}

export function renderHotkeys(): HTMLElement {
  const lockScreen = state.config.settings.lockScreenSupport;
  const rows = state.config.hotkeys.map((hk, i) =>
    h(
      "tr",
      {},
      h(
        "td",
        {},
        h(
          "button",
          {
            class: `btn chord${recording === hk ? " recording" : ""}`,
            onclick: () => {
              recording = hk;
              window.addEventListener("keydown", onRecordKey, true);
              changed();
            },
          },
          recording === hk ? t("hotkeys.press") : hk.chord,
        ),
      ),
      h("td", {}, status(hk.chord)),
      h(
        "td",
        {},
        h(
          "div",
          { class: "row" },
          select(
            [
              ["toggle", t("hotkeys.toggle")],
              ["cycle", t("hotkeys.cycle")],
              ["apply", t("hotkeys.apply")],
              ["recover", t("tray.recover")],
            ],
            hk.action.type,
            (v) => changeType(hk, v),
          ),
          groupPicker(hk.action),
        ),
      ),
      onLinux()
        ? null
        : h(
            "td",
            {},
            h("label", { class: "row", title: lockScreen ? "" : t("hotkeys.lockScreenOff") }, toggle(hk.onLockScreen, (v) => ((hk.onLockScreen = v), changed()), !lockScreen), t("hotkeys.lockScreen")),
          ),
      h(
        "td",
        {},
        h(
          "button",
          { class: "btn icon-only", "aria-label": t("editor.remove"), onclick: () => (state.config.hotkeys.splice(i, 1), changed()) },
          icon(""),
        ),
      ),
    ),
  );
  const ids = state.config.groups.map((g) => g.id);
  return h(
    "div",
    { class: "page" },
    h("h2", {}, t("nav.hotkeys")),
    h("p", { class: "muted" }, t("hotkeys.intro")),
    state.config.hotkeys.length
      ? h(
          "table",
          { class: "list" },
          h("thead", {}, h("tr", {}, h("th", {}, t("hotkeys.chord")), h("th", {}, t("hotkeys.status")), h("th", {}, t("hotkeys.action")), onLinux() ? null : h("th", {}), h("th", {}))),
          h("tbody", {}, ...rows),
        )
      : h("p", { class: "muted" }, t("hotkeys.none")),
    h(
      "button",
      {
        class: "btn",
        disabled: ids.length === 0,
        onclick: () => {
          state.config.hotkeys.push({
            // GNOME keeps Ctrl+Alt+F1…F12 for switching to the text consoles.
            chord: onLinux() ? "Super+Shift+F11" : "Ctrl+Alt+F11",
            action: ids.length >= 2 ? { type: "toggle", groups: ids.slice(0, 2) } : { type: "apply", group: ids[0] },
            onLockScreen: false,
          });
          changed();
        },
      },
      icon(""),
      t("hotkeys.add"),
    ),
  );
}

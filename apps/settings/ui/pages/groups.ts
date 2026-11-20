// Groups: list (reorder, duplicate, delete with undo) and the group editor.

import { api, SCALE_STEPS, type Action, type DisplaySetting, type Group, type ModeOption, type Rotation } from "../api";
import { field, h, icon, ltr, numberInput, select, toggle } from "../dom";
import { t } from "../i18n";
import { iconTile } from "../icons";
import { applyAndReport, saveNow, toast } from "../main";
import { changed, dirty, monitorName, monitorNumber, onLinux, reload, state } from "../state";
import { layoutCanvas, makePrimary, size } from "./canvas";

const modeCache = new Map<string, ModeOption[] | string>();

/** The displays changed: modes are listed again (a display may be on now). */
export function forgetModes(): void {
  modeCache.clear();
}

function selected(): Group | undefined {
  return state.config.groups.find((g) => g.id === state.selectedGroup) ?? state.config.groups[0];
}

function summary(g: Group): string {
  return g.displays
    .filter((d) => d.enabled)
    .sort((a, b) => Number(b.position.x === 0 && b.position.y === 0) - Number(a.position.x === 0 && a.position.y === 0))
    .map((d) => monitorName(d.monitor))
    .join(" · ");
}

function tile(iconName: string): HTMLElement {
  return iconTile(iconName);
}

/** "Choose file…": the system's Open dialog; the file is copied into the icons folder. */
async function pickIcon(g: Group): Promise<void> {
  try {
    const value = await api.pickIcon(t("icons.pickTitle"), t("icons.filter"));
    if (value) {
      g.icon = value;
      changed();
    }
  } catch (e) {
    toast(String(e), "error");
  }
}

/** "Identify": each display shows its number and name for a few seconds. */
async function identify(): Promise<void> {
  try {
    await api.identify();
  } catch (e) {
    toast(String(e) || t("common.failed"), "error");
  }
}

function requireSaved(): boolean {
  if (dirty()) {
    toast(t("common.saveFirst"), "error");
    return false;
  }
  return true;
}

/**
 * A name for capturing what is on screen now that is not taken: "Desktop" for the
 * first group, otherwise the displays that are on (e.g. "SAMSUNG").
 */
function suggestedName(): string {
  const shown = state.status?.displays ?? [];
  const base =
    state.config.groups.length === 0 || shown.length === 0
      ? t("groups.captureDefault")
      : shown.map((d) => (d.monitorKey ? monitorName(d.monitorKey) : d.name)).join(" + ");
  return uniqueName(base);
}

async function capture(): Promise<void> {
  if (!requireSaved()) return;
  const name = window.prompt(t("groups.captureName"), suggestedName())?.trim();
  if (!name) return;
  // Capturing under an existing name replaces that group's layout: never by accident.
  const existing = state.config.groups.find((g) => g.name.trim().toLowerCase() === name.toLowerCase());
  if (existing && !window.confirm(t("groups.updateConfirm", { name: existing.name }))) return;
  const result = await api.cli({ kind: "capture", name });
  toast(result.output || (result.ok ? t("common.done") : t("common.failed")), result.ok ? "info" : "error");
  await reload();
  const created = state.config.groups.find((g) => g.name.trim().toLowerCase() === name.toLowerCase());
  if (created) state.selectedGroup = created.id;
  changed();
}

/**
 * "New group": an empty group to build by hand, for a layout that is not on screen
 * (e.g. the TV while you sit at the desk). It is saved with the other changes.
 */
function newGroup(): void {
  const g: Group = {
    id: crypto.randomUUID(),
    name: uniqueName(t("groups.newName")),
    icon: "builtin:monitor",
    othersPolicy: "disable",
    displays: [],
    audio: { enabled: false, endpoint: null, volumePercent: null },
    actions: { enabled: false, allowFromUri: false, before: [], after: [] },
    safety: { confirmSeconds: 15 },
  };
  state.config.groups.push(g);
  state.selectedGroup = g.id;
  state.selectedDisplay = 0;
  changed();
  // Ready to type the name.
  requestAnimationFrame(() => document.querySelector<HTMLInputElement>(".group-name")?.select());
}

/**
 * A display added to a group starts from the best settings known for that monitor:
 * the same monitor in another group (with its captured timings and scale), else what
 * it shows now, else its native mode from the EDID. The first display is the primary.
 */
function newDisplay(g: Group, key: string): DisplaySetting {
  const right = g.displays.length ? Math.max(...g.displays.map((d) => d.position.x + size(d).w)) : 0;
  const elsewhere = state.config.groups.flatMap((o) => o.displays).find((d) => d.monitor === key);
  if (elsewhere) {
    return { ...structuredClone(elsewhere), enabled: true, position: { x: right, y: 0 } };
  }
  const active = state.status?.displays.find((d) => d.monitorKey === key);
  const connected = state.status?.connected.find((c) => c.monitorKey === key || c.newMonitor?.key === key);
  const mode = active
    ? { width: active.width, height: active.height, refreshMilliHz: active.refreshMilliHz ?? 60000 }
    : (connected?.preferred ?? { width: 1920, height: 1080, refreshMilliHz: 60000 });
  return {
    monitor: key,
    enabled: true,
    mode: { ...mode, refreshPolicy: "bestAtOrBelow" },
    position: { x: right, y: 0 },
    rotation: "landscape",
    scalePercent: null,
    hdr: null,
    captured: null,
  };
}

/** Replaces the group's displays with what is on screen now (same group, same id). */
async function updateFromScreen(g: Group): Promise<void> {
  if (!requireSaved()) return;
  if (!window.confirm(t("groups.updateConfirm", { name: g.name }))) return;
  const result = await api.cli({ kind: "capture", name: g.name });
  toast(result.ok ? t("groups.updated", { name: g.name }) : result.output || t("common.failed"), result.ok ? "info" : "error");
  await reload();
  changed();
}

/** What is on screen right now; kept current as displays change. */
function screenNow(): HTMLElement | null {
  const status = state.status;
  if (!status) return null;
  const current = state.config.groups.find((g) => g.id === state.currentGroup);
  const off = status.connected.filter((c) => !c.active);
  return h(
    "div",
    { class: "screen-now", role: "status" },
    h("span", { class: "label" }, t("groups.screenNow")),
    ...status.displays.map((d) =>
      h(
        "span",
        { class: "chip" },
        h("b", {}, d.monitorKey ? monitorName(d.monitorKey) : d.name),
        ltr(`${d.width}×${d.height}${d.refreshMilliHz ? ` · ${Math.round(d.refreshMilliHz / 1000)} Hz` : ""}`),
      ),
    ),
    off.length
      ? h("span", { class: "muted" }, t("groups.screenOff", { names: off.map((c) => (c.monitorKey ? monitorName(c.monitorKey) : c.name)).join(t("common.listSeparator")) }))
      : null,
    h("span", { class: "spacer" }),
    current ? h("span", { class: "badge ok" }, t("groups.screenIsGroup", { name: current.name })) : h("span", { class: "badge" }, t("groups.screenNoGroup")),
    current ? null : h("button", { class: "btn accent", onclick: () => void capture() }, icon(""), t("groups.saveScreen")),
  );
}

function uniqueName(base: string): string {
  const taken = (n: string) => state.config.groups.some((g) => g.name.trim().toLowerCase() === n.toLowerCase());
  if (!taken(base)) return base;
  for (let i = 2; ; i++) if (!taken(`${base} ${i}`)) return `${base} ${i}`;
}

function duplicate(g: Group): void {
  const copy = structuredClone(g);
  copy.id = crypto.randomUUID();
  copy.name = uniqueName(t("groups.copyName", { name: g.name }));
  const index = state.config.groups.indexOf(g);
  state.config.groups.splice(index + 1, 0, copy);
  state.selectedGroup = copy.id;
  changed();
}

function remove(g: Group): void {
  const index = state.config.groups.indexOf(g);
  state.config.groups.splice(index, 1);
  state.selectedGroup = state.config.groups[Math.max(0, index - 1)]?.id ?? null;
  changed();
  toast(t("groups.deleted", { name: g.name }), "info", {
    label: t("groups.undo"),
    run: () => {
      state.config.groups.splice(index, 0, g);
      state.selectedGroup = g.id;
      changed();
    },
  });
}

function groupList(): HTMLElement {
  let dragIndex = -1;
  const list = h("div", { class: "group-list", role: "listbox", "aria-label": t("nav.groups") });
  state.config.groups.forEach((g, i) => {
    const item = h(
      "button",
      {
        class: "group-item",
        role: "option",
        draggable: true,
        "aria-selected": String(g.id === selected()?.id),
        onclick: () => {
          state.selectedGroup = g.id;
          state.selectedDisplay = 0;
          changed();
        },
        ondragstart: () => {
          dragIndex = i;
          item.classList.add("dragging");
        },
        ondragend: () => item.classList.remove("dragging"),
        ondragover: (e: DragEvent) => e.preventDefault(),
        ondrop: (e: DragEvent) => {
          e.preventDefault();
          if (dragIndex < 0 || dragIndex === i) return;
          const [moved] = state.config.groups.splice(dragIndex, 1);
          state.config.groups.splice(i, 0, moved);
          changed();
        },
      },
      tile(g.icon),
      h("span", { class: "meta" }, h("span", {}, g.name), h("small", {}, summary(g))),
      h("span", { class: "spacer" }),
      g.id === state.currentGroup ? h("span", { class: "badge ok" }, t("groups.onScreen")) : null,
    );
    list.append(item);
  });
  return list;
}

function loadModes(monitor: string): void {
  if (modeCache.has(monitor)) return;
  modeCache.set(monitor, "loading");
  api
    .monitorModes(state.config, monitor)
    .then((modes) => modeCache.set(monitor, modes))
    .catch((e) => modeCache.set(monitor, String(e)))
    .finally(() => changed());
}

function modeControls(d: DisplaySetting): HTMLElement[] {
  loadModes(d.monitor);
  const modes = modeCache.get(d.monitor);
  if (Array.isArray(modes) && modes.length) {
    const resolutions = [...new Set(modes.map((m) => `${m.width}×${m.height}`))];
    const current = `${d.mode.width}×${d.mode.height}`;
    if (!resolutions.includes(current)) resolutions.unshift(current);
    const rates = [...new Set(modes.filter((m) => m.width === d.mode.width && m.height === d.mode.height).map((m) => m.refreshHz))];
    const currentHz = Math.round(d.mode.refreshMilliHz / 1000);
    if (!rates.includes(currentHz)) rates.unshift(currentHz);
    return [
      field(
        t("editor.resolution"),
        select(
          resolutions.map((r) => [r, ltr(r)] as [string, string]),
          current,
          (v) => {
            const [w, hgt] = v.split("×").map(Number);
            d.mode.width = w;
            d.mode.height = hgt;
            changed();
          },
        ),
      ),
      field(
        t("editor.refresh"),
        select(
          rates.map((r) => [String(r), ltr(`${r} Hz`)] as [string, string]),
          String(currentHz),
          (v) => {
            d.mode.refreshMilliHz = Number(v) * 1000;
            changed();
          },
        ),
      ),
    ];
  }
  const hint = typeof modes === "string" && modes !== "loading" ? (modes === "off" ? t("editor.modesWhenOn") : modes) : undefined;
  return [
    field(
      t("editor.resolution"),
      h(
        "span",
        { class: "row mode-size" },
        numberInput(d.mode.width, (v) => ((d.mode.width = v), changed()), { min: 640 }),
        "×",
        numberInput(d.mode.height, (v) => ((d.mode.height = v), changed()), { min: 480 }),
      ),
      hint,
    ),
    field(
      t("editor.refresh"),
      numberInput(Math.round(d.mode.refreshMilliHz / 1000), (v) => ((d.mode.refreshMilliHz = v * 1000), changed()), { min: 1 }),
    ),
  ];
}

function inspector(g: Group, index: number): HTMLElement {
  const d = g.displays[index];
  if (!d) return h("div");
  const primary = d.enabled && d.position.x === 0 && d.position.y === 0;
  const rotations: [Rotation, string][] = [
    ["landscape", t("rotation.landscape")],
    ["portrait", t("rotation.portrait")],
    ["landscapeFlipped", t("rotation.landscapeFlipped")],
    ["portraitFlipped", t("rotation.portraitFlipped")],
  ];
  return h(
    "div",
    { class: "card", style: "margin-block-start:12px" },
    h(
      "div",
      { class: "row" },
      h("strong", {}, monitorName(d.monitor)),
      primary ? h("span", { class: "badge ok" }, t("editor.primary")) : null,
      h("span", { class: "spacer" }),
      h("label", { class: "row" }, t("editor.enabled"), toggle(d.enabled, (v) => ((d.enabled = v), changed()))),
      h(
        "button",
        { class: "btn", disabled: primary || !d.enabled, onclick: () => (makePrimary(g, index), changed()) },
        t("editor.makePrimary"),
      ),
      h(
        "button",
        {
          class: "btn danger",
          disabled: g.displays.length <= 1,
          onclick: () => {
            g.displays.splice(index, 1);
            state.selectedDisplay = 0;
            changed();
          },
        },
        t("editor.remove"),
      ),
    ),
    h(
      "div",
      { class: "inspector" },
      ...modeControls(d),
      field(t("editor.rotation"), select(rotations, d.rotation, (v) => ((d.rotation = v), changed()))),
      field(
        t("editor.scale"),
        select(
          [["", t("editor.leaveAsIs")], ...SCALE_STEPS.map((s) => [String(s), `${s}%`] as [string, string])],
          d.scalePercent === null ? "" : String(d.scalePercent),
          (v) => ((d.scalePercent = v === "" ? null : Number(v)), changed()),
        ),
      ),
      // GNOME's display service has no HDR setting yet; a Windows value is kept as is.
      onLinux()
        ? null
        : field(
            t("editor.hdr"),
            select(
              [
                ["", t("editor.leaveAsIs")],
                ["on", t("common.on")],
                ["off", t("common.off")],
              ],
              d.hdr === null ? "" : d.hdr ? "on" : "off",
              (v) => ((d.hdr = v === "" ? null : v === "on"), changed()),
            ),
          ),
      field(
        t("editor.position"),
        h(
          "span",
          { class: "row" },
          numberInput(d.position.x, (v) => ((d.position = { ...d.position, x: v }), changed())),
          numberInput(d.position.y, (v) => ((d.position = { ...d.position, y: v }), changed())),
        ),
      ),
    ),
  );
}

function actionsEditor(list: Action[]): HTMLElement {
  const wrap = h("div", { class: "issues" });
  list.forEach((a, i) => {
    const target =
      a.kind === "open"
        ? h("input", { type: "text", value: a.target, placeholder: "steam://open/bigpicture", style: "flex:1", onchange: (e: Event) => ((a.target = (e.target as HTMLInputElement).value), changed()) })
        : h("input", { type: "text", value: a.path, placeholder: "C:\\Program Files\\Kodi\\kodi.exe", style: "flex:1", onchange: (e: Event) => ((a.path = (e.target as HTMLInputElement).value), changed()) });
    wrap.append(
      h(
        "div",
        { class: "row" },
        select(
          [
            ["open", t("actions.open")],
            ["program", t("actions.program")],
          ],
          a.kind,
          (v) => {
            list[i] = v === "open" ? { kind: "open", target: "" } : { kind: "program", path: "", args: [] };
            changed();
          },
        ),
        target,
        a.kind === "program"
          ? h("input", {
              type: "text",
              value: a.args.join(" | "),
              placeholder: t("actions.argsHint"),
              onchange: (e: Event) => {
                a.args = (e.target as HTMLInputElement).value.split("|").map((x) => x.trim()).filter(Boolean);
                changed();
              },
            })
          : null,
        h("button", { class: "btn icon-only", "aria-label": t("editor.remove"), onclick: () => (list.splice(i, 1), changed()) }, icon("\uE74D")),
      ),
    );
  });
  wrap.append(h("div", {}, h("button", { class: "btn", onclick: () => (list.push({ kind: "open", target: "" }), changed()) }, icon("\uE710"), t("actions.add"))));
  return wrap;
}

function moreOptions(g: Group): HTMLElement {
  const audio = g.audio;
  return h(
    "details",
    { class: "more" },
    h("summary", {}, t("editor.more")),
    h(
      "div",
      { class: "inspector" },
      field(
        t("editor.others"),
        select(
          [
            ["disable", t("editor.othersOff")],
            ["leave", t("editor.othersLeave")],
          ],
          g.othersPolicy,
          (v) => ((g.othersPolicy = v), changed()),
        ),
      ),
      onLinux()
        ? // GNOME asks for a fixed 20 seconds; only whether it asks can be chosen.
          field(
            t("editor.confirm"),
            toggle(g.safety.confirmSeconds > 0, (v) => ((g.safety.confirmSeconds = v ? 15 : 0), changed())),
            t("editor.confirmHint"),
          )
        : field(t("editor.confirm"), numberInput(g.safety.confirmSeconds, (v) => ((g.safety.confirmSeconds = Math.max(0, Math.min(120, v))), changed()), { min: 0, max: 120 }), t("editor.confirmHint")),
    ),
    // The sound output is switched on Windows only so far.
    onLinux() ? null : h("h3", {}, t("editor.audio")),
    onLinux()
      ? null
      : h(
          "div",
          { class: "inspector" },
          field(t("editor.audioSwitch"), toggle(audio.enabled, (v) => ((audio.enabled = v), changed()))),
          field(
            t("editor.audioEndpoint"),
            h("input", {
              type: "text",
              value: audio.endpoint?.nameContains ?? "",
              placeholder: "SAMSUNG",
              onchange: (e: Event) => {
                const v = (e.target as HTMLInputElement).value.trim();
                audio.endpoint = v ? { nameContains: v } : null;
                changed();
              },
            }),
          ),
          field(
            t("editor.volume"),
            h(
              "span",
              { class: "row" },
              toggle(audio.volumePercent !== null, (v) => ((audio.volumePercent = v ? 50 : null), changed())),
              audio.volumePercent !== null
                ? h("input", { type: "range", min: 0, max: 100, value: String(audio.volumePercent), onchange: (e: Event) => ((audio.volumePercent = Number((e.target as HTMLInputElement).value)), changed()) })
                : null,
              audio.volumePercent !== null ? `${audio.volumePercent}%` : t("editor.leaveAsIs"),
            ),
          ),
        ),
    h("h3", {}, t("editor.actions")),
    h(
      "div",
      { class: "row" },
      h("label", { class: "row" }, toggle(g.actions.enabled, (v) => ((g.actions.enabled = v), changed())), t("actions.enabled")),
      h("label", { class: "row" }, toggle(g.actions.allowFromUri, (v) => ((g.actions.allowFromUri = v), changed())), t("actions.fromUri")),
    ),
    h("p", { class: "muted" }, t("actions.after")),
    actionsEditor(g.actions.after),
  );
}

function editor(g: Group): HTMLElement {
  const index = Math.min(state.selectedDisplay, g.displays.length - 1);
  const missing = Object.keys(state.config.monitors).filter((k) => !g.displays.some((d) => d.monitor === k));
  // Connected monitors that are in no group yet (e.g. a TV that was off when the
  // layouts were captured): adding one also saves its identity in the config.
  const fresh = (state.status?.connected ?? []).flatMap((c) => (c.newMonitor && !state.config.monitors[c.newMonitor.key] ? [c.newMonitor] : []));
  const groupIndex = state.config.groups.indexOf(g);
  const issues = state.issues.filter((i) => i.path === `groups[${groupIndex}]` || i.path.startsWith(`groups[${groupIndex}].`) || i.path.startsWith(`groups[${groupIndex}][`));

  return h(
    "section",
    { "aria-label": g.name },
    h(
      "div",
      { class: "row" },
      h("input", {
        type: "text",
        class: "group-name",
        value: g.name,
        "aria-label": t("groups.rename"),
        style: "font-size:20px;font-weight:600;min-inline-size:260px",
        onchange: (e: Event) => ((g.name = (e.target as HTMLInputElement).value), changed()),
      }),
      ...Object.keys(state.icons).map((name) =>
        h(
          "button",
          {
            class: "btn icon-only",
            "aria-pressed": String(g.icon === `builtin:${name}`),
            "aria-label": t(`icons.${name}`),
            title: t(`icons.${name}`),
            onclick: () => ((g.icon = `builtin:${name}`), changed()),
          },
          tile(`builtin:${name}`),
        ),
      ),
      g.icon.startsWith("file:")
        ? h("button", { class: "btn icon-only", "aria-pressed": "true", "aria-label": t("icons.custom"), title: t("icons.custom") }, tile(g.icon))
        : null,
      h("button", { class: "btn", onclick: () => void pickIcon(g) }, icon(""), t("icons.choose")),
      h("span", { class: "spacer" }),
      h(
        "button",
        {
          class: "btn accent",
          onclick: async () => {
            if (!requireSaved()) return;
            await applyAndReport(g);
          },
        },
        icon("\uE768"),
        t("groups.test"),
      ),
      h("button", { class: "btn", title: t("groups.updateFromScreenHint"), onclick: () => void updateFromScreen(g) }, icon("\uE72C"), t("groups.updateFromScreen")),
      h(
        "button",
        {
          class: "btn",
          onclick: async () => {
            if (!requireSaved()) return;
            const r = await api.cli({ kind: "validate", id: g.id });
            try {
              const report = JSON.parse(r.output) as { wouldUse: string | null };
              toast(report.wouldUse ? t("groups.valid", { strategy: report.wouldUse }) : t("groups.invalid"), report.wouldUse ? "info" : "error");
            } catch {
              toast(r.output || t("common.failed"), r.ok ? "info" : "error");
            }
          },
        },
        t("groups.validate"),
      ),
      h("button", { class: "btn", onclick: () => duplicate(g) }, t("groups.duplicate")),
      h("button", { class: "btn danger", onclick: () => remove(g) }, t("groups.delete")),
    ),
    h("div", { style: "margin-block-start:12px" }),
    g.displays.length
      ? layoutCanvas(
          g,
          index,
          (i) => {
            state.selectedDisplay = i;
            changed();
          },
          (ok) => {
            if (!ok) toast(t("editor.overlap"), "error");
            changed();
          },
        )
      : h("div", { class: "canvas empty-group" }, h("p", {}, t("editor.noDisplays"))),
    h(
      "div",
      { class: "display-tabs" },
      ...g.displays.map((d, i) =>
        h(
          "button",
          {
            class: "btn",
            "aria-pressed": String(i === index),
            onclick: () => {
              state.selectedDisplay = i;
              changed();
            },
          },
          h("b", { class: "num" }, String(monitorNumber(d.monitor))),
          monitorName(d.monitor),
        ),
      ),
      h("button", { class: "btn", title: t("editor.identifyHint"), onclick: () => void identify() }, icon(""), t("editor.identify")),
      missing.length || fresh.length
        ? select(
            [
              ["", t("editor.addDisplay")],
              ...missing.map((k) => [k, monitorName(k)] as [string, string]),
              ...fresh.map((n) => [`new:${n.key}`, t("editor.newMonitor", { name: n.entry.displayName })] as [string, string]),
            ],
            "",
            (choice) => {
              if (!choice) return;
              let key = choice;
              if (choice.startsWith("new:")) {
                const n = fresh.find((x) => `new:${x.key}` === choice);
                if (!n) return;
                state.config.monitors[n.key] = n.entry;
                key = n.key;
              }
              g.displays.push(newDisplay(g, key));
              state.selectedDisplay = g.displays.length - 1;
              changed();
            },
          )
        : null,
    ),
    g.displays.length ? inspector(g, index) : null,
    issues.length ? h("div", { class: "issues" }, ...issues.map((i) => h("div", { class: `issue ${i.severity}` }, h("strong", {}, i.severity === "error" ? "!" : "i"), i.message))) : null,
    moreOptions(g),
  );
}

export function renderGroups(): HTMLElement {
  const g = selected();
  const header = h(
    "div",
    { class: "row" },
    h("h2", {}, t("nav.groups")),
    h("span", { class: "spacer" }),
    dirty() ? h("button", { class: "btn accent", onclick: () => void saveNow() }, t("common.save")) : null,
    h("button", { class: "btn", onclick: () => newGroup() }, icon("\uE710"), t("groups.new")),
    h("button", { class: "btn", onclick: () => void capture() }, icon("\uE722"), t("groups.capture")),
  );
  if (!g) {
    return h("div", { class: "page" }, header, screenNow(), h("p", { class: "muted" }, t("groups.empty")));
  }
  return h("div", { class: "page" }, header, screenNow(), h("div", { class: "groups-layout" }, groupList(), editor(g)));
}

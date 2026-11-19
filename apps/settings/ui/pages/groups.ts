// Groups: list (reorder, duplicate, delete with undo) and the group editor.

import { api, SCALE_STEPS, type Action, type DisplaySetting, type Group, type ModeOption, type Rotation } from "../api";
import { field, h, icon, numberInput, select, toggle } from "../dom";
import { t } from "../i18n";
import { saveNow, toast } from "../main";
import { changed, dirty, iconGlyph, monitorName, reload, state } from "../state";
import { layoutCanvas, makePrimary } from "./canvas";

const modeCache = new Map<string, ModeOption[] | string>();

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
  const { glyph, color } = iconGlyph(iconName);
  return h("span", { class: "tile", style: `background:${color}` }, icon(glyph));
}

function requireSaved(): boolean {
  if (dirty()) {
    toast(t("common.saveFirst"), "error");
    return false;
  }
  return true;
}

async function capture(): Promise<void> {
  if (!requireSaved()) return;
  const name = window.prompt(t("groups.captureName"), t("groups.captureDefault"));
  if (!name?.trim()) return;
  const result = await api.cli({ kind: "capture", name: name.trim() });
  toast(result.output || (result.ok ? t("common.done") : t("common.failed")), result.ok ? "info" : "error");
  await reload();
  const created = state.config.groups.find((g) => g.name.trim().toLowerCase() === name.trim().toLowerCase());
  if (created) state.selectedGroup = created.id;
  changed();
}

function uniqueName(base: string): string {
  const taken = (n: string) => state.config.groups.some((g) => g.name.trim().toLowerCase() === n.toLowerCase());
  if (!taken(base)) return base;
  for (let i = 2; ; i++) if (!taken(`${base} ${i}`)) return `${base} ${i}`;
}

function duplicate(g: Group): void {
  const copy = structuredClone(g);
  copy.id = crypto.randomUUID();
  copy.name = uniqueName(`${g.name} (copy)`);
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
          resolutions.map((r) => [r, r] as [string, string]),
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
          rates.map((r) => [String(r), `${r} Hz`] as [string, string]),
          String(currentHz),
          (v) => {
            d.mode.refreshMilliHz = Number(v) * 1000;
            changed();
          },
        ),
      ),
    ];
  }
  const hint = typeof modes === "string" && modes !== "loading" ? modes : undefined;
  return [
    field(
      t("editor.resolution"),
      h(
        "span",
        { class: "row" },
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
      field(
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
      field(t("editor.confirm"), numberInput(g.safety.confirmSeconds, (v) => ((g.safety.confirmSeconds = Math.max(0, Math.min(120, v))), changed()), { min: 0, max: 120 }), t("editor.confirmHint")),
    ),
    h("h3", {}, t("editor.audio")),
    h(
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
            "aria-label": name,
            title: name,
            style: g.icon === `builtin:${name}` ? "border-color:var(--accent)" : "",
            onclick: () => ((g.icon = `builtin:${name}`), changed()),
          },
          tile(`builtin:${name}`),
        ),
      ),
      h("span", { class: "spacer" }),
      h(
        "button",
        {
          class: "btn accent",
          onclick: async () => {
            if (!requireSaved()) return;
            await api.applyGroup(g.id);
            toast(t("groups.testing", { name: g.name }));
          },
        },
        icon("\uE768"),
        t("groups.test"),
      ),
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
    layoutCanvas(
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
    ),
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
          `${i + 1}. ${monitorName(d.monitor)}`,
        ),
      ),
      missing.length
        ? select(
            [["", t("editor.addDisplay")], ...missing.map((k) => [k, monitorName(k)] as [string, string])],
            "",
            (key) => {
              if (!key) return;
              const right = Math.max(...g.displays.map((d) => d.position.x + d.mode.width));
              g.displays.push({
                monitor: key,
                enabled: true,
                mode: { width: 1920, height: 1080, refreshMilliHz: 60000, refreshPolicy: "bestAtOrBelow" },
                position: { x: right, y: 0 },
                rotation: "landscape",
                scalePercent: null,
                hdr: null,
                captured: null,
              });
              state.selectedDisplay = g.displays.length - 1;
              changed();
            },
          )
        : null,
    ),
    inspector(g, index),
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
    h("button", { class: "btn", onclick: () => void capture() }, icon("\uE722"), t("groups.capture")),
  );
  if (!g) {
    return h("div", { class: "page" }, header, h("p", { class: "muted" }, t("groups.empty")));
  }
  return h("div", { class: "page" }, header, h("div", { class: "groups-layout" }, groupList(), editor(g)));
}

// Automatic switching (ARCHITECTURE.md §15.1): switch to a group when a program starts
// or a game controller connects, and back when it ends. The agent does the watching.

import { api, type Trigger } from "../api";
import { field, h, icon, numberInput, select, toggle } from "../dom";
import { t } from "../i18n";
import { toast } from "../main";
import { changed, state } from "../state";

/** Programs with a window open, loaded when asked for. */
let programs: string[] | null = null;

async function loadPrograms(): Promise<void> {
  try {
    programs = await api.runningPrograms();
  } catch (e) {
    toast(String(e), "error");
  }
  changed();
}

function groupName(id: string): string {
  return state.config.groups.find((g) => g.id === id)?.name ?? "?";
}

/** A new trigger switches to a group other than the one on screen, usually the TV. */
function newTrigger(): Trigger {
  const groups = state.config.groups;
  const target = groups.find((g) => g.id !== state.currentGroup) ?? groups[0];
  return {
    id: crypto.randomUUID(),
    enabled: true,
    when: { type: "program", names: [] },
    group: target?.id ?? "",
    switchBack: true,
    revertDelayMs: 5000,
    confirm: false,
  };
}

function summary(tr: Trigger): string {
  const group = groupName(tr.group);
  if (tr.when.type === "controller") return t("triggers.summaryController", { group });
  const names = tr.when.names.length ? tr.when.names.join(", ") : "…";
  return t("triggers.summaryProgram", { names, group });
}

function programFields(tr: Trigger): HTMLElement | null {
  const when = tr.when;
  if (when.type !== "program") return null;
  const names = h("input", {
    type: "text",
    dir: "auto",
    value: when.names.join(", "),
    placeholder: "eldenring.exe",
    style: "flex:1;min-inline-size:200px",
    onchange: (e: Event) => {
      when.names = (e.target as HTMLInputElement).value
        .split(/[,;]/)
        .map((s) => s.trim())
        .filter(Boolean);
      changed();
    },
  });
  const pick =
    programs === null
      ? h("button", { class: "btn", onclick: () => void loadPrograms() }, icon(""), t("triggers.listPrograms"))
      : h(
          "span",
          { class: "row" },
          select(
            [["", t("triggers.addRunning")], ...programs.map((p) => [p, p] as [string, string])],
            "",
            (v) => {
              if (v && !when.names.some((n) => n.toLowerCase() === v)) {
                when.names.push(v);
                changed();
              }
            },
          ),
          h("button", { class: "btn icon-only", title: t("diagnostics.refresh"), "aria-label": t("diagnostics.refresh"), onclick: () => void loadPrograms() }, icon("")),
        );
  const title = h("input", {
    type: "text",
    dir: "auto",
    value: when.windowTitle ?? "",
    placeholder: "Big Picture",
    onchange: (e: Event) => {
      const v = (e.target as HTMLInputElement).value.trim();
      when.windowTitle = v || null;
      changed();
    },
  });
  return h(
    "div",
    { class: "inspector" },
    h("div", { class: "wide" }, field(t("triggers.programs"), h("span", { class: "row" }, names, pick), t("triggers.programsHint"))),
    field(t("triggers.windowTitle"), title, t("triggers.windowTitleHint")),
  );
}

function card(tr: Trigger, index: number): HTMLElement {
  const groups = state.config.groups.map((g) => [g.id, g.name] as [string, string]);
  const issues = state.issues.filter((i) => i.path === `triggers[${index}]` || i.path.startsWith(`triggers[${index}].`));
  return h(
    "div",
    { class: `card trigger${tr.enabled ? "" : " disabled"}` },
    h(
      "div",
      { class: "row" },
      toggle(tr.enabled, (v) => ((tr.enabled = v), changed())),
      h("strong", {}, summary(tr)),
      h("span", { class: "spacer" }),
      h(
        "button",
        {
          class: "btn danger",
          onclick: () => {
            state.config.triggers.splice(index, 1);
            changed();
          },
        },
        t("groups.delete"),
      ),
    ),
    h(
      "div",
      { class: "inspector" },
      field(
        t("triggers.when"),
        select(
          [
            ["program", t("triggers.program")],
            ["controller", t("triggers.controller")],
          ],
          tr.when.type,
          (v) => {
            tr.when = v === "program" ? { type: "program", names: [] } : { type: "controller" };
            changed();
          },
        ),
      ),
      field(t("triggers.switchTo"), select(groups, tr.group, (v) => ((tr.group = v), changed()))),
    ),
    programFields(tr),
    h(
      "div",
      { class: "inspector" },
      field(
        t("triggers.switchBack"),
        h(
          "span",
          { class: "row" },
          toggle(tr.switchBack, (v) => ((tr.switchBack = v), changed())),
          tr.switchBack
            ? numberInput(
                tr.revertDelayMs / 1000,
                (v) => {
                  tr.revertDelayMs = Math.round(Math.min(600, Math.max(0, v)) * 1000);
                  changed();
                },
                { min: 0, max: 600, step: 1, title: t("triggers.revertDelay"), style: "inline-size:72px" },
              )
            : null,
          tr.switchBack ? t("triggers.seconds") : null,
        ),
        t("triggers.switchBackHint"),
      ),
      field(t("triggers.confirm"), toggle(tr.confirm, (v) => ((tr.confirm = v), changed())), t("triggers.confirmHint")),
    ),
    issues.length
      ? h("div", { class: "issues" }, ...issues.map((i) => h("div", { class: `issue ${i.severity}` }, h("strong", {}, i.severity === "error" ? "!" : "i"), i.message)))
      : null,
  );
}

export function renderTriggers(): HTMLElement {
  const triggers = state.config.triggers;
  return h(
    "div",
    { class: "page" },
    h("h2", {}, t("nav.triggers")),
    h("p", { class: "muted" }, t("triggers.intro")),
    ...triggers.map((tr, i) => card(tr, i)),
    triggers.length ? null : h("p", { class: "muted" }, t("triggers.none")),
    h(
      "button",
      {
        class: "btn",
        disabled: state.config.groups.length === 0,
        onclick: () => {
          triggers.push(newTrigger());
          changed();
        },
      },
      icon(""),
      t("triggers.add"),
    ),
    h("p", { class: "muted" }, t("triggers.edgeHint")),
  );
}

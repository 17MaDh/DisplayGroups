// A tiny element builder: h("button", { class: "btn", onclick: fn }, "Save").

import { glyphSvg } from "./glyphs";

type Child = Node | string | number | null | undefined | false;
type Props = Record<string, unknown>;

const PROPERTY_KEYS = new Set(["value", "checked", "disabled", "selected", "min", "max", "step", "type", "placeholder", "draggable"]);

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Props = {}, ...children: Child[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  apply(el, props);
  append(el, children);
  return el;
}

const SVG_NS = "http://www.w3.org/2000/svg";

export function s<K extends keyof SVGElementTagNameMap>(tag: K, props: Props = {}, ...children: Child[]): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2), value as EventListener);
    } else {
      el.setAttribute(key, String(value));
    }
  }
  append(el, children);
  return el;
}

function apply(el: HTMLElement, props: Props): void {
  for (const [key, value] of Object.entries(props)) {
    if (value === undefined || value === null) continue;
    if (key === "class") {
      el.className = String(value);
    } else if (key === "style") {
      el.style.cssText = String(value);
    } else if (key === "dataset") {
      Object.assign(el.dataset, value as Record<string, string>);
    } else if (key.startsWith("on") && typeof value === "function") {
      el.addEventListener(key.slice(2), value as EventListener);
    } else if (PROPERTY_KEYS.has(key)) {
      (el as unknown as Record<string, unknown>)[key] = value;
    } else if (value === true) {
      el.setAttribute(key, "");
    } else if (value !== false) {
      el.setAttribute(key, String(value));
    }
  }
}

function append(el: Element, children: Child[]): void {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : String(c));
  }
}

/**
 * Keeps technical text ("3840×2160 · 120 Hz", coordinates) in left-to-right order on
 * right-to-left pages, using Unicode isolates (works in <option> and SVG text too).
 */
export function ltr(text: string): string {
  return `⁦${text}⁩`;
}

let drawnGlyphs = false;

/** Systems without the Segoe Fluent Icons font get the drawings in glyphs.ts. */
export function useDrawnGlyphs(on: boolean): void {
  drawnGlyphs = on;
  document.documentElement.dataset.glyphs = on ? "svg" : "font";
}

/** A Segoe Fluent Icons glyph (or its drawing where the font is missing). */
export function icon(codepoint: string, label?: string): HTMLSpanElement {
  const content = drawnGlyphs ? glyphSvg(codepoint) : codepoint;
  return h("span", { class: "glyph", "aria-hidden": label ? undefined : "true", "aria-label": label }, content);
}

export function field(label: string, control: HTMLElement, hint?: string): HTMLLabelElement {
  return h("label", { class: "field" }, h("span", { class: "field-label" }, label), control, hint ? h("span", { class: "field-hint" }, hint) : null);
}

export function toggle(checked: boolean, onchange: (value: boolean) => void, disabled = false): HTMLInputElement {
  return h("input", {
    type: "checkbox",
    class: "switch",
    role: "switch",
    checked,
    disabled,
    onchange: (e: Event) => onchange((e.target as HTMLInputElement).checked),
  });
}

export function select<T extends string>(options: [T, string][], value: T, onchange: (value: T) => void): HTMLSelectElement {
  return h(
    "select",
    { onchange: (e: Event) => onchange((e.target as HTMLSelectElement).value as T) },
    ...options.map(([v, label]) => h("option", { value: v, selected: v === value }, label)),
  );
}

export function numberInput(value: number, onchange: (value: number) => void, props: Props = {}): HTMLInputElement {
  return h("input", {
    type: "number",
    value: String(value),
    ...props,
    onchange: (e: Event) => {
      const n = Number((e.target as HTMLInputElement).value);
      if (Number.isFinite(n)) onchange(n);
    },
  });
}

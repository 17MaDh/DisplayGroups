// The 2D layout canvas: drag displays, snap to edges, keep the primary at (0,0).

import type { DisplaySetting, Group } from "../api";
import { ltr, s } from "../dom";
import { monitorName, monitorNumber } from "../state";

const SNAP_PX = 8;

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function size(d: DisplaySetting): { w: number; h: number } {
  const portrait = d.rotation === "portrait" || d.rotation === "portraitFlipped";
  return portrait ? { w: d.mode.height, h: d.mode.width } : { w: d.mode.width, h: d.mode.height };
}

function box(d: DisplaySetting): Box {
  const { w, h } = size(d);
  return { x: d.position.x, y: d.position.y, w, h };
}

function overlaps(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** Snaps `moving` to edges of `others` when within `threshold` desktop pixels. */
export function snap(moving: Box, others: Box[], threshold: number): { x: number; y: number } {
  let best = { dx: threshold + 1, x: moving.x, dy: threshold + 1, y: moving.y };
  for (const o of others) {
    for (const x of [o.x - moving.w, o.x + o.w, o.x, o.x + o.w - moving.w]) {
      const d = Math.abs(x - moving.x);
      if (d < best.dx) best = { ...best, dx: d, x };
    }
    for (const y of [o.y - moving.h, o.y + o.h, o.y, o.y + o.h - moving.h]) {
      const d = Math.abs(y - moving.y);
      if (d < best.dy) best = { ...best, dy: d, y };
    }
  }
  return {
    x: Math.round(best.dx <= threshold ? best.x : moving.x),
    y: Math.round(best.dy <= threshold ? best.y : moving.y),
  };
}

/** Moves display `index` to (x, y). Returns false (and changes nothing) on overlap. */
export function moveDisplay(group: Group, index: number, x: number, y: number): boolean {
  const d = group.displays[index];
  const candidate = { ...box(d), x, y };
  const blocked = group.displays.some((o, i) => i !== index && o.enabled && d.enabled && overlaps(candidate, box(o)));
  if (blocked) return false;
  const wasPrimary = d.enabled && d.position.x === 0 && d.position.y === 0;
  d.position = { x, y };
  if (wasPrimary) {
    // Keep the primary at the origin: shift everything else instead.
    for (const o of group.displays) {
      o.position = { x: o.position.x - x, y: o.position.y - y };
    }
  }
  return true;
}

/** Makes display `index` the primary by moving the origin to it. */
export function makePrimary(group: Group, index: number): void {
  const { x, y } = group.displays[index].position;
  for (const o of group.displays) {
    o.position = { x: o.position.x - x, y: o.position.y - y };
  }
}

export function layoutCanvas(
  group: Group,
  selected: number,
  onSelect: (index: number) => void,
  onMoved: (moved: boolean) => void,
): HTMLElement {
  const boxes = group.displays.map(box);
  const minX = Math.min(...boxes.map((b) => b.x));
  const minY = Math.min(...boxes.map((b) => b.y));
  const maxX = Math.max(...boxes.map((b) => b.x + b.w));
  const maxY = Math.max(...boxes.map((b) => b.y + b.h));
  const pad = Math.max(maxX - minX, maxY - minY) * 0.15 + 200;
  const viewBox = [minX - pad, minY - pad, maxX - minX + 2 * pad, maxY - minY + 2 * pad];
  // Screen geometry is not mirrored on right-to-left pages, and neither is the text on
  // it: WebKitGTK ignores Unicode isolates in SVG text, so the direction is set here.
  const svg = s("svg", { viewBox: viewBox.join(" "), preserveAspectRatio: "xMidYMid meet", role: "img", direction: "ltr", style: "direction:ltr" });
  const fontScale = viewBox[2] / 900;

  group.displays.forEach((d, i) => {
    const b = boxes[i];
    const primary = d.enabled && d.position.x === 0 && d.position.y === 0;
    const rate = (d.mode.refreshMilliHz / 1000).toFixed(d.mode.refreshMilliHz % 1000 ? 2 : 0);
    const g = s(
      "g",
      {
        class: `display${i === selected ? " selected" : ""}${d.enabled ? "" : " disabled"}`,
        transform: `translate(${b.x} ${b.y})`,
        tabindex: 0,
        role: "button",
        "aria-label": monitorName(d.monitor),
      },
      s("rect", { width: b.w, height: b.h, rx: 24 * fontScale, ry: 24 * fontScale, "vector-effect": "non-scaling-stroke" }),
      // The same number "Identify" shows on the real screen.
      s("text", { class: "num", x: 28 * fontScale, y: 52 * fontScale, style: `font-size:${32 * fontScale}px` }, String(monitorNumber(d.monitor))),
      s(
        "text",
        { x: b.w / 2, y: b.h / 2 - 6 * fontScale, "text-anchor": "middle", style: `font-size:${18 * fontScale}px` },
        monitorName(d.monitor) + (primary ? " ★" : ""),
      ),
      s(
        "text",
        { class: "sub", x: b.w / 2, y: b.h / 2 + 20 * fontScale, "text-anchor": "middle", style: `font-size:${15 * fontScale}px` },
        ltr(`${d.mode.width}×${d.mode.height} · ${rate} Hz`),
      ),
    );
    g.addEventListener("pointerdown", (e) => startDrag(e, i, g));
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") onSelect(i);
    });
    svg.append(g);
  });

  function toSvg(e: PointerEvent): { x: number; y: number } {
    const m = svg.getScreenCTM();
    if (!m) return { x: 0, y: 0 };
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(m.inverse());
    return { x: p.x, y: p.y };
  }

  function startDrag(e: PointerEvent, index: number, g: SVGGElement): void {
    e.preventDefault();
    // Selection happens on release (when the pointer did not move), so dragging is
    // never interrupted by a re-render.
    const start = toSvg(e);
    const origin = boxes[index];
    const others = boxes.filter((_, i) => i !== index && group.displays[i].enabled);
    const threshold = SNAP_PX * (viewBox[2] / Math.max(svg.clientWidth, 1));
    let current = { x: origin.x, y: origin.y };
    let moved = false;
    g.classList.add("moving");
    svg.setPointerCapture(e.pointerId);

    const move = (ev: PointerEvent) => {
      const p = toSvg(ev);
      const raw = { ...origin, x: origin.x + p.x - start.x, y: origin.y + p.y - start.y };
      current = snap(raw, others, threshold);
      moved = moved || Math.abs(p.x - start.x) > threshold / 2 || Math.abs(p.y - start.y) > threshold / 2;
      g.setAttribute("transform", `translate(${current.x} ${current.y})`);
    };
    const up = () => {
      svg.removeEventListener("pointermove", move);
      svg.removeEventListener("pointerup", up);
      svg.removeEventListener("pointercancel", up);
      g.classList.remove("moving");
      if (moved) {
        onMoved(moveDisplay(group, index, current.x, current.y));
      } else {
        onSelect(index);
      }
    };
    svg.addEventListener("pointermove", move);
    svg.addEventListener("pointerup", up);
    svg.addEventListener("pointercancel", up);
  }

  const wrap = document.createElement("div");
  wrap.className = "canvas";
  wrap.append(svg);
  return wrap;
}

// "Identify" on Linux: the settings app opens this page full screen on each display.
// The display's number, name and mode come from the window's initialization script.

import { getCurrentWindow } from "@tauri-apps/api/window";

interface Label {
  number: number;
  name: string;
  mode: string;
}

const label = (window as unknown as { __DG_IDENTIFY?: Label }).__DG_IDENTIFY;

function fill(selector: string, text: string): void {
  const el = document.querySelector(selector);
  if (el) el.textContent = text;
}

if (label) {
  fill(".number", String(label.number));
  fill(".name", label.name);
  fill(".mode", label.mode);
}

const close = () => void getCurrentWindow().close().catch(() => undefined);
window.addEventListener("click", close);
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") close();
});

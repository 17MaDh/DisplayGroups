// Icons for systems without the Segoe Fluent Icons font (Linux). Each is drawn here as
// simple 24×24 strokes and keyed by the Segoe code point the page already uses, so the
// same `icon("")` call works everywhere.

/** A path, or a circle as [cx, cy, r]. */
type Shape = string | [number, number, number];

const SHAPES: Record<string, Shape[]> = {
  // Add
  "": ["M12 5v14", "M5 12h14"],
  // Settings (sliders)
  "": ["M4 7h9", "M17 7h3", "M4 17h3", "M11 17h9", [15, 7, 2], [9, 17, 2]],
  // Link
  "": ["M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1", "M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1"],
  // List of programs
  "": ["M9 6h11", "M9 12h11", "M9 18h11", "M4.5 6h.01", "M4.5 12h.01", "M4.5 18h.01"],
  // Capture (camera)
  "": ["M4 8h3l2-2.5h6L17 8h3a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z", [12, 13, 3.5]],
  // Refresh
  "": ["M19.5 12a7.5 7.5 0 1 1-2.2-5.3", "M19.5 4.5v4h-4"],
  // Remove (bin)
  "": ["M4 7h16", "M9 7V4.5h6V7", "M6.5 7l1 13h9l1-13", "M10 11v5.5", "M14 11v5.5"],
  // Keyboard
  "": ["M3 6.5h18v11H3z", "M7 10h.01", "M10 10h.01", "M13 10h.01", "M16 10h.01", "M8 14h8"],
  // Test (play)
  "": ["M8 5.5v13l10.5-6.5z"],
  // Display / TV
  "": ["M3.5 5h17v11h-17z", "M9 20h6", "M12 16v4"],
  // Desktop
  "": ["M3.5 4.5h17v12h-17z", "M8 20h8", "M12 16.5V20", "M7 8h1.5", "M7 11h1.5"],
  // Gaming (controller)
  "": [
    "M7.5 8h9A4.5 4.5 0 0 1 21 12.5l-.4 3a2.4 2.4 0 0 1-4.1 1.3L14.8 15H9.2l-1.7 1.8a2.4 2.4 0 0 1-4.1-1.3L3 12.5A4.5 4.5 0 0 1 7.5 8z",
    "M7.5 10.5v3",
    "M6 12h3",
    "M15.5 11h.01",
    "M17.5 13h.01",
  ],
  // Work (briefcase)
  "": ["M3.5 8h17v11h-17z", "M9 8V5.5h6V8", "M3.5 13h17"],
  // Open folder
  "": ["M3.5 6.5h5.5l2 2h9.5v10h-17z"],
  // Import
  "": ["M12 4v10", "M8 10l4 4 4-4", "M5 14.5V19h14v-4.5"],
  // Choose a picture
  "": ["M4 5h16v14H4z", "M4 16l5-5 4 4 2.5-2.5L20 17", [15.5, 9, 1.5]],
  // App grid
  "": ["M4.5 4.5h5.5v5.5H4.5z", "M14 4.5h5.5v5.5H14z", "M4.5 14h5.5v5.5H4.5z", "M14 14h5.5v5.5H14z"],
  // Automatic switching (bolt)
  "": ["M13 3L5 13.5h6L10 21l8-10.5h-6z"],
  // Diagnostics (pulse)
  "": ["M3 12h4l2.5-6 5 12 2.5-6h4"],
  // Export
  "": ["M12 14V4", "M8 8l4-4 4 4", "M5 14.5V19h14v-4.5"],
};

const SVG = "http://www.w3.org/2000/svg";

/** The drawing for a Segoe code point, or null when there is none. */
export function glyphSvg(codepoint: string): SVGSVGElement | null {
  const shapes = SHAPES[codepoint];
  if (!shapes) return null;
  const svg = document.createElementNS(SVG, "svg");
  for (const [k, v] of Object.entries({
    viewBox: "0 0 24 24",
    width: "1em",
    height: "1em",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "1.75",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
  })) {
    svg.setAttribute(k, v);
  }
  for (const shape of shapes) {
    if (typeof shape === "string") {
      const path = document.createElementNS(SVG, "path");
      path.setAttribute("d", shape);
      svg.append(path);
    } else {
      const circle = document.createElementNS(SVG, "circle");
      circle.setAttribute("cx", String(shape[0]));
      circle.setAttribute("cy", String(shape[1]));
      circle.setAttribute("r", String(shape[2]));
      svg.append(circle);
    }
  }
  return svg;
}

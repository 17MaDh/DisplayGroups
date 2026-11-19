// Typed wrappers around the Rust commands in src-tauri/src/main.rs.
// The config types mirror crates/dg-model (camelCase JSON). Objects are edited in
// place and sent back whole, so fields this UI does not know about are preserved.

import { invoke } from "@tauri-apps/api/core";

export type Rotation = "landscape" | "portrait" | "landscapeFlipped" | "portraitFlipped";
export type Theme = "system" | "dark" | "light";

export interface MonitorIdentity {
  pnpId: string;
  productCode: string;
  serial?: string | null;
  friendlyName: string;
  connectorHint?: string | null;
  [extra: string]: unknown;
}

export interface MonitorEntry {
  displayName: string;
  identity: MonitorIdentity;
  [extra: string]: unknown;
}

export interface DisplaySetting {
  monitor: string;
  enabled: boolean;
  mode: { width: number; height: number; refreshMilliHz: number; refreshPolicy?: "exact" | "bestAtOrBelow" };
  position: { x: number; y: number };
  rotation: Rotation;
  scalePercent: number | null;
  hdr: boolean | null;
  captured?: unknown;
  [extra: string]: unknown;
}

export type Action =
  | { kind: "program"; path: string; args: string[]; workingDir?: string | null }
  | { kind: "open"; target: string };

export interface Group {
  id: string;
  name: string;
  icon: string;
  othersPolicy: "disable" | "leave";
  displays: DisplaySetting[];
  audio: { enabled: boolean; endpoint: { nameContains: string } | null; volumePercent: number | null; [extra: string]: unknown };
  actions: { enabled: boolean; allowFromUri: boolean; before: Action[]; after: Action[]; [extra: string]: unknown };
  safety: { confirmSeconds: number };
  [extra: string]: unknown;
}

export type HotkeyAction =
  | { type: "apply"; group: string }
  | { type: "toggle"; groups: string[] }
  | { type: "cycle"; groups: string[] }
  | { type: "recover" };

export interface Hotkey {
  chord: string;
  action: HotkeyAction;
  onLockScreen: boolean;
  [extra: string]: unknown;
}

export type TriggerWhen =
  | { type: "program"; names: string[]; windowTitle?: string | null }
  | { type: "controller" };

/** Automatic switching (ARCHITECTURE.md §15.1). */
export interface Trigger {
  id: string;
  enabled: boolean;
  when: TriggerWhen;
  group: string;
  switchBack: boolean;
  revertDelayMs: number;
  confirm: boolean;
  [extra: string]: unknown;
}

export interface Settings {
  theme: Theme;
  language: string;
  startWithWindows: boolean;
  startMinimized: boolean;
  osd: { enabled: boolean; durationMs: number };
  disconnectFallback: { enabled: boolean; group: string | null; debounceMs: number };
  lockScreenSupport: boolean;
  logLevel: "error" | "warn" | "info" | "debug" | "trace";
  [extra: string]: unknown;
}

export interface Config {
  schemaVersion: number;
  monitors: Record<string, MonitorEntry>;
  groups: Group[];
  hotkeys: Hotkey[];
  triggers: Trigger[];
  settings: Settings;
  [extra: string]: unknown;
}

export interface Issue {
  severity: "error" | "warning";
  path: string;
  message: string;
}

export interface ConfigState {
  config: Config;
  issues: Issue[];
  configPath: string;
  logsDir: string;
  portable: boolean;
}

export interface Platform {
  mica: boolean;
  build: number;
  version: string;
  /** English first, then each `locales\<code>.json`, named in its own language. */
  languages: { code: string; name: string }[];
  /** What the "system" language setting resolves to. */
  systemLanguage: string;
}

export interface ActiveDisplay {
  name: string;
  monitorKey: string | null;
  width: number;
  height: number;
  x: number;
  y: number;
  refreshMilliHz: number | null;
  primary: boolean;
}

export interface ConnectedMonitor {
  name: string;
  monitorKey: string | null;
  active: boolean;
  /** A monitor not in the config yet: the key and entry it would be saved under. */
  newMonitor?: { key: string; entry: MonitorEntry };
  /** Native mode from the EDID (Windows lists modes only for displays that are on). */
  preferred?: { width: number; height: number; refreshMilliHz: number };
}

export interface Status {
  displays: ActiveDisplay[];
  connected: ConnectedMonitor[];
}

export interface HotkeyProbe {
  valid: boolean;
  canonical: string | null;
  error: string | null;
  free: boolean | null;
  reserved: string | null;
  known: string[];
  agentRunning: boolean;
}

export interface ModeOption {
  width: number;
  height: number;
  refreshHz: number;
}

export interface CliResult {
  ok: boolean;
  code: number;
  output: string;
}

export type CliCommand =
  | { kind: "capture"; name: string }
  | { kind: "validate"; id: string }
  | { kind: "shortcut"; id: string; desktop: boolean; startMenu: boolean }
  | { kind: "startup"; on: boolean }
  | { kind: "protocol"; on: boolean }
  | { kind: "identify" };

export const api = {
  platform: () => invoke<Platform>("platform"),
  locale: (language: string) => invoke<Record<string, string>>("locale", { language }),
  loadConfig: () => invoke<ConfigState>("load_config"),
  saveConfig: (config: Config) => invoke<{ saved: boolean; issues: Issue[] }>("save_config", { config }),
  importConfig: (text: string) => invoke<{ saved: boolean; issues: Issue[] }>("import_config", { text }),
  exportConfig: () => invoke<string>("export_config"),
  openLogs: () => invoke<void>("open_logs"),
  probeHotkey: (chord: string) => invoke<HotkeyProbe>("probe_hotkey", { chord }),
  integration: () => invoke<{ startup: boolean; protocol: boolean }>("integration_state"),
  displayStatus: (config: Config) => invoke<[Status, string | null]>("display_status", { config }),
  monitorModes: (config: Config, monitor: string) => invoke<ModeOption[]>("monitor_modes", { config, monitor }),
  cli: (command: CliCommand) => invoke<CliResult>("run_cli", { command }),
  /** Switches for real (with the countdown) and returns the CLI exit code once done. */
  applyGroup: (id: string) => invoke<number>("apply_group", { id }),
  builtinIcons: () => invoke<Record<string, string>>("builtin_icons"),
  /** Windows "Open" dialog for a .ico/.png; returns the value for `group.icon`, or null. */
  pickIcon: (title: string, filter: string) => invoke<string | null>("pick_icon", { title, filter }),
  /** A custom (`file:`) icon as a data: URL. */
  iconImage: (icon: string) => invoke<string>("icon_image", { icon }),
  /** Programs with a window open now (lower-case .exe names), for triggers. */
  runningPrograms: () => invoke<string[]>("running_programs"),
};

export const SCALE_STEPS = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

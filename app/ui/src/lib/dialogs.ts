// Native file dialogs (Tauri dialog plugin), with browser-preview fallbacks.

import { isTauri } from "../api";

const MEDIA_EXT = ["mp4", "mov", "m4v", "mkv", "mxf", "m2ts", "mts", "avi", "webm", "dv", "mpg", "ts", "3gp", "braw", "r3d", "ari", "crm", "nev"];

export async function pickMedia(): Promise<string[]> {
  if (!isTauri) return ["D:/Footage/Card_A001"];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ multiple: true, title: "Add media", filters: [{ name: "Video", extensions: MEDIA_EXT }] });
  return r === null ? [] : Array.isArray(r) ? r : [r];
}

export async function pickFolder(): Promise<string | null> {
  if (!isTauri) return "D:/Footage";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, title: "Add a folder of media" });
  return typeof r === "string" ? r : null;
}

export async function pickProjectToOpen(): Promise<string | null> {
  if (!isTauri) return "Demo.clippro";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ title: "Open project", filters: [{ name: "ClipPro project", extensions: ["clippro"] }] });
  return typeof r === "string" ? r : null;
}

export async function pickExportPath(defaultName: string, ext: string, label: string): Promise<string | null> {
  if (!isTauri) return `D:/Exports/${defaultName}.${ext}`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  const r = await save({ title: "Export selects", defaultPath: `${defaultName}.${ext}`, filters: [{ name: label, extensions: [ext] }] });
  if (!r) return null;
  return r.toLowerCase().endsWith(`.${ext}`) ? r : `${r}.${ext}`;
}

export async function pickSettingsFile(save: boolean): Promise<string | null> {
  if (!isTauri) return "D:/clippro-settings.json";
  const { open, save: saveDlg } = await import("@tauri-apps/plugin-dialog");
  const filters = [{ name: "ClipPro settings", extensions: ["json"] }];
  if (save) {
    const r = await saveDlg({ title: "Export settings", defaultPath: "clippro-settings.json", filters });
    return r ? (r.toLowerCase().endsWith(".json") ? r : `${r}.json`) : null;
  }
  const r = await open({ title: "Import settings", filters });
  return typeof r === "string" ? r : null;
}

export async function pickPresetFile(): Promise<string | null> {
  if (!isTauri) return "D:/Presets/demo.toml";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ title: "Import a preset", filters: [{ name: "ClipPro preset", extensions: ["toml"] }] });
  return typeof r === "string" ? r : null;
}

export async function pickPresetExport(name: string): Promise<string | null> {
  if (!isTauri) return `D:/Presets/${name}.toml`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  const r = await save({ title: "Export preset", defaultPath: `${name}.toml`, filters: [{ name: "ClipPro preset", extensions: ["toml"] }] });
  if (!r) return null;
  return r.toLowerCase().endsWith(".toml") ? r : `${r}.toml`;
}

export async function pickProxy(): Promise<string | null> {
  if (!isTauri) return "D:/Footage/Proxy/clip_Proxy.mov";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ title: "Link a proxy", filters: [{ name: "Video", extensions: MEDIA_EXT }] });
  return typeof r === "string" ? r : null;
}

export async function pickLut(): Promise<string | null> {
  if (!isTauri) return "D:/LUTs/Demo.cube";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ title: "Choose a viewer LUT", filters: [{ name: "Cube LUT", extensions: ["cube"] }] });
  return typeof r === "string" ? r : null;
}

export async function pickNewProject(): Promise<string | null> {
  if (!isTauri) return "Untitled.clippro";
  const { save } = await import("@tauri-apps/plugin-dialog");
  const r = await save({ title: "New project", defaultPath: "Untitled.clippro", filters: [{ name: "ClipPro project", extensions: ["clippro"] }] });
  if (!r) return null;
  return r.toLowerCase().endsWith(".clippro") ? r : `${r}.clippro`;
}

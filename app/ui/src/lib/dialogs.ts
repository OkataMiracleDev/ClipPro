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

export async function pickNewProject(): Promise<string | null> {
  if (!isTauri) return "Untitled.clippro";
  const { save } = await import("@tauri-apps/plugin-dialog");
  const r = await save({ title: "New project", defaultPath: "Untitled.clippro", filters: [{ name: "ClipPro project", extensions: ["clippro"] }] });
  if (!r) return null;
  return r.toLowerCase().endsWith(".clippro") ? r : `${r}.clippro`;
}

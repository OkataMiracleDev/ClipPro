import { afterEach, describe, expect, it } from "vitest";
import { ACTIONS, actionFor, bindingsWith, comboOf, conflicts, customOnly, setBindings } from "./keys";

const ev = (key: string, mods: Partial<{ ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }> = {}) =>
  ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods }) as KeyboardEvent;

afterEach(() => setBindings(null));

describe("keyboard bindings", () => {
  it("names key events consistently", () => {
    expect(comboOf(ev(" "))).toBe("Space");
    expect(comboOf(ev("z", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+Z");
    expect(comboOf(ev("Z", { shiftKey: true }))).toBe("Shift+Z");
    expect(comboOf(ev("[", {}))).toBe("[");
    expect(comboOf(ev("{", { shiftKey: true }))).toBe("{");
    expect(comboOf(ev("ArrowLeft", { shiftKey: true }))).toBe("Shift+ArrowLeft");
    expect(comboOf(ev("Shift", { shiftKey: true }))).toBeNull();
  });

  it("defaults are unique and resolve", () => {
    const keys = ACTIONS.map((a) => a.key);
    expect(new Set(keys).size).toBe(keys.length);
    expect(actionFor(ev("l"))).toBe("shuttle_forward");
    expect(actionFor(ev("l", { ctrlKey: true }))).toBe("loop");
    expect(actionFor(ev("ArrowRight", { shiftKey: true }))).toBe("forward_second");
  });

  it("rebinding takes effect and conflicts are detected", () => {
    const b = bindingsWith({ zebras: "X" });
    expect(conflicts(b, "peaking", "X")).toEqual(["zebras"]);
    expect(conflicts(b, "zebras", "X")).toEqual([]);
    expect(customOnly(b)).toEqual({ zebras: "X" });
    setBindings({ zebras: "X" });
    expect(actionFor(ev("x"))).toBe("zebras");
    expect(actionFor(ev("z"))).toBeNull();
  });
});

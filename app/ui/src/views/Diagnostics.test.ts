import { describe, expect, it } from "vitest";
import { levelOf } from "./Diagnostics";

describe("levelOf", () => {
  it("reads the level column", () => {
    expect(levelOf("2026-10-30 09:00:01Z INFO  started")).toBe("INFO");
    expect(levelOf("2026-10-30 09:00:01Z WARN  analysis job 3 failed")).toBe("WARN");
    expect(levelOf("2026-10-30 09:00:01Z PANIC boom")).toBe("PANIC");
    expect(levelOf("random text")).toBeNull();
  });
});

import { describe, expect, it } from "vitest";
import { formatBytes } from "./OffloadSheet";

describe("formatBytes", () => {
  it("uses decimal units like drive labels", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1_800_000_000)).toBe("1.8 GB");
    expect(formatBytes(772_000_000)).toBe("772 MB");
    expect(formatBytes(3.2e12)).toBe("3.2 TB");
  });
});

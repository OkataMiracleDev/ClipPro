import { describe, expect, it } from "vitest";
import { recordTc } from "./TimelineReview";

describe("recordTc", () => {
  it("formats non-drop timecode", () => {
    expect(recordTc(90_000, 25, false)).toBe("01:00:00:00");
    expect(recordTc(90_000 + 113, 25, false)).toBe("01:00:04:13");
  });
  it("skips the dropped labels in 29.97 drop-frame", () => {
    expect(recordTc(107_892, 29.97, true)).toBe("01:00:00;00");
    expect(recordTc(1800, 29.97, true)).toBe("00:01:00;02");
    expect(recordTc(17_982, 29.97, true)).toBe("00:10:00;00");
  });
});

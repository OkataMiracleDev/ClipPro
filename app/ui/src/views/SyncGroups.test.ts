import { describe, expect, it } from "vitest";
import { signedSeconds } from "./SyncGroups";

describe("signedSeconds", () => {
  it("signs offsets and hides rounding noise", () => {
    expect(signedSeconds(3_480_000)).toBe("+3.48 s");
    expect(signedSeconds(-40_000)).toBe("−0.04 s");
    expect(signedSeconds(1_000)).toBe("0.00 s");
  });
});

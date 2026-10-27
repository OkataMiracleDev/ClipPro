import { describe, expect, it } from "vitest";
import { mockBackend } from "../mock";
import { diffSettings } from "./diff";

describe("diffSettings", () => {
  it("lists only what changed, in words", async () => {
    const base = await mockBackend.presetSettings("All-Rounder");
    const infos = await mockBackend.submetrics();
    expect(diffSettings(base, structuredClone(base), infos)).toEqual([]);
    const cur = structuredClone(base);
    cur.score_lo = 0.6;
    cur.weights = [0, 40, 40, 20, 0];
    cur.subs[0] = { ...cur.subs[0]!, enabled: false };
    cur.tiers[5] = "force_exclude";
    const d = diffSettings(base, cur, infos);
    expect(d.map((c) => c.what)).toEqual(["Score threshold", "Focus weight", "Exposure weight", "Motion weight", "Audio weight", "Sharpness", "Shaky flag"]);
    expect(d[0]).toEqual({ what: "Score threshold", from: "55–100", to: "60–100" });
    expect(d.find((c) => c.what === "Sharpness")?.to).toBe("off");
  });
});

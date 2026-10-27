import vectors from "../../../../testdata/typed_values.json";
import { formatDuration, parseCount, parseDuration, parseScore } from "./typed";

describe("typed values match the Rust parser (shared vectors)", () => {
  it("durations", () => {
    for (const [input, want] of vectors.duration as [string, number | null][]) {
      const got = parseDuration(input);
      expect(got.ok, input).toBe(true);
      if (got.ok) {
        if (want === null) expect(got.value, input).toBeNull();
        else expect(got.value, input).toBeCloseTo(want, 9);
      }
    }
    for (const input of vectors.duration_invalid) expect(parseDuration(input).ok, input).toBe(false);
  });

  it("counts", () => {
    for (const [input, want] of vectors.count as [string, number | null][]) {
      const got = parseCount(input);
      expect(got.ok && got.value, input).toBe(want === null ? null : want);
    }
    for (const input of vectors.count_invalid) expect(parseCount(input).ok, input).toBe(false);
  });

  it("scores", () => {
    for (const [input, want] of vectors.score as [string, number][]) {
      const got = parseScore(input);
      expect(got.ok, input).toBe(true);
      if (got.ok) expect(got.value).toBeCloseTo(want, 9);
    }
    for (const input of vectors.score_invalid) expect(parseScore(input).ok, input).toBe(false);
  });

  it("formats", () => {
    expect(formatDuration(null)).toBe("none");
    expect(formatDuration(4)).toBe("4s");
    expect(formatDuration(0.25)).toBe("0.25s");
    expect(formatDuration(90)).toBe("1:30");
    expect(formatDuration(3723)).toBe("1:02:03");
  });
});

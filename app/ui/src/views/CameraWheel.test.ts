import { describe, expect, it } from "vitest";
import { puckToCamera } from "./CameraWheel";

describe("camera wheel", () => {
  it("maps the puck to direction and strictness (0° = pan right, 90° = tilt up)", () => {
    expect(puckToCamera(75 + 58, 75)).toEqual({ radius: 1, direction: 0 });
    expect(puckToCamera(75, 75 - 29)).toEqual({ radius: 0.5, direction: 90 });
    expect(puckToCamera(75 - 58, 75).direction).toBe(180);
    expect(puckToCamera(76, 75).radius).toBe(0); // centre detent: any direction
  });
});

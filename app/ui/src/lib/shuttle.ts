// JKL shuttle (SPEC §15): L plays forward and doubles on repeat, J the same in reverse, K stops.
// Rates are ±1, ±2, ±4, ±8.

export type ShuttleKey = "j" | "k" | "l";

export function shuttle(key: ShuttleKey, playing: boolean, rate: number): { playing: boolean; rate: number } {
  switch (key) {
    case "k":
      return { playing: false, rate: 1 };
    case "l":
      return { playing: true, rate: playing && rate > 0 ? Math.min(8, rate * 2) : 1 };
    case "j":
      return { playing: true, rate: playing && rate < 0 ? Math.max(-8, rate * 2) : -1 };
  }
}

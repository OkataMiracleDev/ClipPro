/** A finished job in the store's `analysis.jobs` (its clip then counts as done, so it adds no
 *  partial progress). Running jobs hold 0–1. */
export const FINISHED = 2;

/** Progress of the analysis as a whole, over `total` clips of which `done` are done or failed.
 *  A job at 1 has measured every sample and is finishing (temporal pass, audio, saving), which
 *  can take a while on long clips: it counts fully, and `finishing` says that's all that's left. */
export function analysisProgress(jobs: Record<number, number>, done: number, total: number) {
  const active = Object.values(jobs).filter((f) => f < FINISHED);
  const partial = active.reduce((a, f) => a + Math.min(1, f), 0);
  return {
    fraction: total ? Math.min(1, (done + partial) / total) : 0,
    finishing: active.length > 0 && active.every((f) => f >= 1),
  };
}

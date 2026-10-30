# ADR-006: GPU measurement deferred; the CPU path is the reference

- Status: accepted
- Date: 2026-10-30
- Supersedes the "P8 is the remedy" part of ADR-003

## Context
ADR-003 kept the 4× realtime budget for 4K ProRes 422 HQ and named P8 (D3D11 compute for the working image and spatial measures) as the way to reach it. Its own profile of a 4K ProRes sample, which decodes two frames for pair sampling, is:

| Stage | CPU per sample |
|---|---|
| Decode, 2 frames | about 336 ms |
| Working image, 2 frames | about 24 ms |
| Measurement | about 17 ms |

Moving the working image and the measurement to the GPU removes at most about 41 ms of 377 ms, roughly 11%. The 1.9× batch figure would become about 2.1×, not 4×. Decoding sets the ceiling, and D3D11VA can't decode ProRes. 4K H.264 and HEVC, which the GPU does decode, already meet their budgets on the CPU measurement path (5.6× against 5×).

A GPU measurement path would also need:
- a second implementation of every spatial kernel in HLSL;
- the ≥ 99.9% parity work of SPEC §8.3 (floating-point reductions on the GPU differ from the CPU);
- device-removal (TDR) recovery.

That is a large, permanently maintained surface, for about 10% on one codec family.

## Decision
- **The CPU measurement path is the only path in 1.0.** `clippro-gpu` stays an empty placeholder. ANA-8's "GPU measurement" becomes LATER; GPU *decoding* (D3D11VA) stays.
- **Reproducible mode (ANA-12) is the default behaviour.** Measurements are bit-identical across runs, worker counts, decoder thread counts and long-clip sections (`tests/reproducible.rs`). On the dev machine they were also identical with hardware decoding on. Turning off Settings → Performance → Hardware decoding gives the documented guarantee across machines.
- **The 4K ProRes budget is recorded as unmet on the CPU-ref class** (about 1.9× in batches). It is decode-bound. The remedies to use are the Draft or Fast sample rates, analysing proxies (ING-3, ING-7), and a machine with more cores. Revisit GPU measurement only if a profile on target hardware shows measurement, not decode, as the limit.

## Consequences
- No new failure modes from GPU drivers in analysis; results don't depend on the GPU.
- SPEC §22's ProRes row stays a known gap, listed in the RC report.
- If GPU measurement returns later, `tests/reproducible.rs` is the parity harness to extend.

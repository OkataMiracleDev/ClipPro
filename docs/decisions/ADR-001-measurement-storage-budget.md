# ADR-001: Measurement storage budget

- Status: accepted
- Date: 2026-10-25

## Context
SPEC §12.3 estimated about 40 MB per 100 hours of footage, and PLAN P3.2 required "1 h at 4 Hz encodes to ≤ 1 MB". That estimate counted only the ~44 scalar columns. Each stored sample also carries:
- 40–64 tile sharpness values (focus overlay, VIE-2)
- a 16×9 motion-vector grid (motion overlay, VIE-4)
- a 64-bin histogram (cuts, take grouping)
- a pHash

The motion vectors alone are about 4 MB raw per footage hour.

## Measurements
Using the encoding in `clippro-measure/src/codec.rs` (columnar, u16-quantised, zstd level 3) on decoded corpus clips at the Standard rate with pair sampling:

| Clip | Per footage hour |
|---|---|
| long_clip (testsrc2, 720p) | 2.10 MB |
| pan_right | 2.95 MB |
| handheld_shake | 3.31 MB |

## Decision
- Keep every overlay data stream, because the overlays are CORE features.
- The budget is **≤ 4 MB per footage hour at 4 Hz** (about 400 MB for 100 h).
- In memory, the decoded SoA columns stay at about 2.3 MB per hour.
- SPEC §12.3 and PLAN P3.2 are updated to match.

## Consequences
- `.clippro` files for very large projects (100 h+) reach a few hundred MB, which is acceptable for a desktop project file.
- If this becomes a problem, motion vectors can be stored at an 8×5 grid. That would roughly halve the size, at the cost of a coarser motion overlay.

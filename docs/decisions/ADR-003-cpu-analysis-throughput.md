# ADR-003: CPU analysis throughput on 4K intra codecs

- Status: accepted (budget kept; the gap is recorded and P8 is the remedy)
- Date: 2026-10-27

## Context
SPEC §22 asks for at least 4× realtime on 4K ProRes 422 HQ 25p (Standard, 4 Hz) on the CPU-ref machine (i5-1135G7). The dev machine is the same class: i7-1185G7, 4 cores / 8 threads, Iris Xe.

The P8.3 performance pass measured the analysis with test clips rendered by the bundled FFmpeg:
- testsrc2 plus temporal noise.
- ProRes 422 HQ at about 760 Mb/s, a realistic bitrate.
- H.264 from libopenh264: all P-frames, GOP 60. That's a worst case, since no frames can be skipped.

## Measurements
Batches are distinct clips analysed together by `clippro-cli analyze`, the normal case for a card. "Single" is one clip on its own.

| Footage | Before | After | Target (CPU-ref) |
|---|---|---|---|
| 4K ProRes HQ 25p, single 10 s clip | 0.6× | 1.3× | 4× |
| 4K ProRes HQ 25p, batch of 3 | — | 1.9× | 4× |
| 4K H.264 30p (hardware decode), batch of 4 | — | 5.6× | 5× (met) |
| 1080p H.264 30p, Draft, single 60 s | 19× | 17–19× | 25× |

### Fixes in this pass
- **Hardware device only when it can decode the stream.** The decoder opened a D3D11VA device for every codec. Having a device forces single-threaded decoding, so ProRes and DNx decoded on one thread (138 ms per 4K frame). `hw_decodable` now checks the codec's D3D11VA configuration and the DXVA format limits (4:2:0; 8-bit H.264, up to 10-bit HEVC/VP9/AV1).
- **Slice threading for intra codecs.** Sampled decoding seeks between frames, which drains a frame-threading pipeline on every seek. Intra-only streams now slice-thread one frame at a time, using all cores.
- **Skipping unneeded packets for intra codecs.** Packets before the next target are dropped before they reach the decoder.

### Where the time goes (4K ProRes HQ, per sample)
- **Decode.** Two frames, target plus motion pair, at about 168 ms of CPU each. Plain `ffmpeg -benchmark` decodes this file at 36 fps using all cores (utime 42 s for 250 frames).
- **Working image.** About 12 ms per frame: 10-bit 4:2:2 to 960 px.
- **Measurement.** About 17 ms: spatial 9 ms, exposure 2.6 ms, motion 3 ms, other 2 ms.

**Ceiling.** Decoding alone allows at most 36 / 2 = 18 samples/s, which is 4.5× realtime at 4 Hz with pair sampling, before any measurement. The 4× budget therefore leaves almost no CPU for the rest. On this machine it is reachable only if measurement leaves the CPU.

## Decision
- **Keep the SPEC budgets.** The H.264 4K budget is met. The ProRes budget stays as the target for the GPU path.
- **P8 moves the measurement off the CPU,** freeing the cores for decoding:
  - working-image downscale and spatial reductions as D3D11 compute on the iGPU;
  - the CPU fallback stays bit-compatible (P8.2 parity).
- **Documented trade-off.** With pair sampling off, decode cost halves, at the cost of shake and blur accuracy. This is not a default.
- **1080p Draft.** The test clip has no B-frames, so every frame must be decoded. Camera footage (High profile with B-frames) lets the decoder skip non-reference frames. Re-measure on real camera files before treating this gap as real.

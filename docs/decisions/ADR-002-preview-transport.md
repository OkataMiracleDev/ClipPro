# ADR-002: Preview transport

- Status: accepted
- Date: 2026-10-27

## Context
The viewer has to show any frame of any clip quickly enough for scrubbing and real-time playback. WebView2 (Chromium) plays H.264, VP8/9 and AV1 natively, and HEVC when the machine has the extension. It can't decode ProRes, DNxHR/HD, MXF-wrapped intermediates or anything that needs a proxy. Those frames must come from the backend decoder (`clippro-media`) and cross from Rust into the page.

Four transports were measured (PLAN P6.0):

| Transport | Path |
|---|---|
| JPEG `<img>` | `clippro://…/frame/{media}/{us}?w=` → `<img>.decode()` → 2D canvas |
| JPEG fetch + bitmap | the same URL → `fetch().blob()` → `createImageBitmap` → 2D canvas |
| Raw I420 over the protocol | `clippro://…/raw/{media}/{us}?w=` → `fetch().arrayBuffer()` → three R8 WebGL2 textures |
| SharedBuffer I420 | `preview_shared` command → `ICoreWebView2Environment12::CreateSharedBuffer` + `ICoreWebView2_17::PostSharedBufferToScript` → `sharedbufferreceived` → the same three textures |

## Measurements
- **Setup.** `ClipPro --bench-preview <out.json> <clip>…` runs the benchmark inside the real WebView2 (Edge 155) with the window hidden. Each figure is 48 sequential forward frames (25 fps clips), timed end to end: request, decode, conversion, transfer, image decode or GPU upload (forced with `getImageData`/`readPixels`). One request is in flight at a time and there is no decode-ahead.
- **Clips.** ProRes 422 HQ testsrc2 clips rendered with the bundled ffmpeg.
- **Noise.** This laptop drops to about 1.2 GHz under load, so repeat runs vary by about ±20 %.

| Source | Transport | Width | Mean ms | p95 ms | fps | KB/frame |
|---|---|---|---|---|---|---|
| 1080p ProRes | JPEG `<img>` | 960 | 21.9 | 26.4 | 45.6 | 32 |
| | JPEG `<img>` | 1920 | 47.3 | 52.0 | 21.1 | 95 |
| | JPEG fetch + bitmap | 960 | 18.6 | 19.6 | 53.8 | 32 |
| | JPEG fetch + bitmap | 1920 | 40.1 | 43.0 | 25.0 | 95 |
| | Raw I420 protocol | 960 | 14.5 | 15.6 | 69.0 | 759 |
| | Raw I420 protocol | 1920 | 33.8 | 34.8 | 29.6 | 3038 |
| | **SharedBuffer I420** | 960 | **10.5** | 10.9 | **95.1** | 759 |
| | **SharedBuffer I420** | 1920 | **14.6** | 15.3 | **68.4** | 3038 |
| | SharedBuffer + decode-ahead | 960 | 8.3 | 8.7 | 121.2 | 759 |
| | **SharedBuffer + decode-ahead** | 1920 | **11.1** | 11.8 | **89.9** | 3038 |
| 2160p ProRes | JPEG fetch + bitmap | 1920 | 60.9 | 65.1 | 16.4 | 96 |
| | Raw I420 protocol | 1920 | 49.1 | 51.5 | 20.4 | 3038 |
| | SharedBuffer I420 | 960 | 27.9 | 35.0 | 35.9 | 759 |
| | SharedBuffer I420 | 1920 | 29.5 | 32.8 | 33.9 | 3038 |
| | SharedBuffer I420 | 3840 | 48.6 | 55.7 | 20.6 | 12150 |
| | SharedBuffer + decode-ahead | 1920 | 27.4 | 28.2 | 36.5 | 3038 |
| | SharedBuffer + decode-ahead | 3840 | 40.0 | 46.0 | 25.0 | 12150 |

### Backend stage costs at full width
Measured in-process by `preview::tests::frame_transport_throughput` with `CLIPPRO_PERF=1`.

| Stage | 1080p | 2160p |
|---|---|---|
| Decode (ProRes, CPU) | 5.8–7.2 ms | 18.5–26 ms |
| swscale to RGB24 | 4.6–5.7 ms | 17–24 ms |
| JPEG encode (`jpeg-encoder`, SIMD, q85) | 21–27 ms | 77–107 ms |

### What the numbers show
- **JPEG.** Encoding dominates. It is cheap to transfer, but no amount of transfer tuning saves it at full 1080p.
- **Raw over the custom protocol.** The bytes are copied through the WebView2 resource-stream path at about 150 MB/s, so a 3 MB 1080p frame spends about 20 ms in transit.
- **SharedBuffer.** It removes that copy. At 1080p the remaining cost is decode plus scaling, which meets the 60 fps target.
- **Decode-ahead.** Playback requests carry `play={rate}`. After answering, the backend renders the frame the next request will want, so decode overlaps transfer and upload. That lifts 1080p to about 90 fps and full-width 4K ProRes to 25 fps (real time for 25p).
- **4K.** ProRes 4K decode and scaling set the limit, not the transport. Further headroom belongs in P8 (GPU colour conversion, NV12 straight from D3D11VA), not in a different transport.

## Decision
1. **Native `<video>`** through the Tauri asset protocol for clips WebView2 can play: H.264 8-bit 4:2:0 in MP4/MOV, HEVC 4:2:0, and VP8/VP9/AV1, all with AAC, MP3, Opus, Vorbis or FLAC audio. This is the zero-copy, hardware-decoded path, and it brings audio for free.
   - The file is added to the asset scope per clip (`media_source`). The global scope stays empty.
   - If the element reports an error (for example HEVC without the extension), the viewer falls back to frames.
2. **SharedBuffer I420 → WebGL2** for every other clip, and whenever an overlay is on (overlays need pixel access, which the cross-origin `<video>` doesn't give).
   - The shader does the YUV→RGB conversion using the matrix and range from the 24-byte frame header.
   - Overlays (zebras, false colour, focus peaking) run in the same shader.
3. **Raw I420 over `clippro://`** is the fallback when SharedBuffer isn't available (older WebView2 runtimes). It uses the same header and renderer.
4. **JPEG over `clippro://`** for thumbnails, filmstrips and reports (small, cacheable, works in `<img>`).
5. **Native child-HWND swapchain: not pursued.** SharedBuffer meets 60 fps at 1080p, so the PLAN's fallback evaluation isn't triggered. Revisit only if 4K playback still misses its target after P8.

## Consequences
- **Backend.** `VideoDecoder::frame_near` decodes forward during playback (one seek per scrub, not per frame). `to_preview` produces RGB24 or I420 at any width. `PreviewCache` keeps three decoders open.
- **Decode-ahead (done).** A one-frame decode-ahead runs per open decoder. A playback request takes the prefetched frame when it is within one step of the target. Stills always decode the exact frame.
- **Follow-ups (P8).**
  - swscale writing straight into the shared buffer, which saves one memcpy.
  - GPU NV12 output from D3D11VA without the CPU transfer.
- **Audio.** Audio for frame-served clips comes with the P6.1 playback engine (cpal master clock). Until then, frame playback is silent and paced by the wall clock.
- **Re-running.** `ClipPro --bench-preview` stays in the app so the numbers can be re-measured on other machines and runtimes.

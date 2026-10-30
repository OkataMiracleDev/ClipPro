# Analysis

## What gets measured

| Group | Measures |
|---|---|
| Focus | Sharpness, Detail, Focus stability |
| Exposure | Grading flexibility, Flicker, Luma, Contrast, Noise, White-balance cast |
| Motion | Camera motion, Subject motion, Motion stability, and the camera's pan, tilt, zoom and rotation |
| Audio | Loudness, Clean (no clipping), Noise floor, Speech presence, Wind, Hum |

Each clip is decoded once. Frames are measured at a reduced size (at most 960 pixels wide), and letterboxing is detected and ignored.

## Sample rate

Set the default in **Settings → Analysis**:

- **Draft** (1 per second): fastest, for triaging long cards.
- **Fast** (2 per second).
- **Standard** (4 per second): the default.
- **Detailed** (8 per second).
- **Thorough** (16 per second): catches very brief problems.
- **Every frame**: slowest; flags are frame-exact.

With pair sampling on (the default), each sample also measures the next frame, which gives true motion and blur.

## Choosing what to measure

In **Settings → Analysis** you can turn whole groups off (for example, skip Audio on silent drone footage). At least one group must stay on. Clips already analysed keep their measurements.

## Speed

- **Hardware decoding** (Settings → Performance) uses the graphics card where it can, and falls back to the processor.
- **Workers** sets how many clips are analysed at once. Auto uses about half your processor's threads.
- **The cache**: once a file has been analysed in any project, a new project reuses the result instantly. See Settings → Cache.

## Controlling the queue

- **Pause** and **resume** from the title bar. Progress within each clip is kept.
- Right-click a waiting clip and choose **Move to next** to analyse it first. Opening a waiting clip in the viewer does the same.
- If a clip fails, right-click it and choose **Retry**, or **Retry with CPU decoding** when the graphics driver is the problem.

## Proxies

If your footage has proxies, ClipPro can analyse the proxy (faster) and still export the original. Proxies named like the clip with `_Proxy`, `_proxy` or `.proxy` are linked automatically when imported together. You can link one by hand (right-click → **Link proxy…**) or make one (right-click → **Make proxy ›**, see [Ingest](09-ingest.md)).

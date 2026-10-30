# Third-party notices

ClipPro includes the following software. Full lists of the Rust crates and JavaScript packages it is built from, with their licences, are in `THIRD-PARTY-RUST.md` and `THIRD-PARTY-JS.md`.

## FFmpeg (LGPL-3.0)

ClipPro uses FFmpeg libraries (avcodec, avformat, avutil, avfilter, avdevice, swscale, swresample) and the `ffmpeg.exe` program, unmodified, from the BtbN FFmpeg-Builds "n9.0 win64-lgpl-shared" build. They are loaded as separate DLLs, so you may replace them with your own build of the same version. The licence text is in `FFMPEG-LICENSE.txt`.

- FFmpeg source: https://ffmpeg.org/download.html (release 9.0)
- Build scripts: https://github.com/BtbN/FFmpeg-Builds

## ONNX Runtime (MIT)

Copyright (c) Microsoft Corporation. Statically linked through the `ort` crate.

## DirectML (Microsoft DirectML redistributable)

`DirectML.dll` is redistributed unmodified under the Microsoft DirectML licence terms, which allow redistribution of the DLL with an application.

## whisper.cpp / ggml (MIT)

Copyright (c) 2023-2024 The ggml authors. Statically linked through the `whisper-rs` crate.

## AI models (downloaded on request, not bundled)

The models are downloaded only after you turn on local AI in Settings → AI Models. Each comes from its publisher and keeps its licence:

| Model | Licence | Source |
|---|---|---|
| YuNet face detector (2023mar) | MIT | OpenCV Zoo |
| SFace face recogniser (2021dec) | Apache-2.0 | OpenCV Zoo |
| CLIP ViT-B/32, 8-bit ONNX | MIT | OpenAI CLIP; ONNX export by Xenova (Hugging Face) |
| Whisper (tiny to large-v3-turbo), ggml | MIT | OpenAI Whisper; ggml conversion by the whisper.cpp project |
| ECAPA-TDNN 512 (VoxCeleb) | Apache-2.0 | WeSpeaker |

## Fonts (OFL-1.1)

Inter and JetBrains Mono, through the `@fontsource-variable` packages.

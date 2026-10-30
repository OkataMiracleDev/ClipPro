# AI models (SPEC §18, PLAN P12.1)

ClipPro's AI modules are optional and off by default. Nothing is downloaded until the user turns a module on and agrees; each file is checked against the SHA-256 below. The manifest the app uses is `crates/clippro-ai/models.json`.

Approved by the owner on 2026-10-30 (🧑 HC-5).

| Module | Model | Source | Licence | Commercial use | Size |
|---|---|---|---|---|---|
| Faces | YuNet face detector (2023-03) | OpenCV Zoo | MIT | Yes | 0.2 MB |
| Faces | SFace face embeddings (2021-12) | OpenCV Zoo | Apache-2.0 | Yes | 39 MB |
| Shot type, search, aesthetic, thumbnails, takes | CLIP ViT-B/32 image and text encoders, 8-bit ONNX | OpenAI CLIP, export by Xenova on Hugging Face | MIT | Yes | 89 + 65 MB |
| Shot type, search | CLIP tokenizer | OpenAI CLIP | MIT | Yes | 2 MB |
| Transcripts | Whisper tiny, base, small (default), medium, large-v3 turbo, ggml | OpenAI Whisper, conversion by whisper.cpp | MIT | Yes | 78 MB to 1.6 GB |
| Speaker turns | ECAPA-TDNN 512 (VoxCeleb) | WeSpeaker | Apache-2.0 | Yes | 25 MB |

Notes:
- The runtime is ONNX Runtime 1.31 (MIT) through the `ort` crate, using DirectML (MIT) on the GPU when available, otherwise the CPU. whisper.cpp (MIT) runs transcripts.
- The aesthetic measure is CLIP-based: the similarity of each frame to "a beautiful, well-composed photograph" against "a dull, poorly composed photograph". It replaces a LAION aesthetic head, whose weights would need a PyTorch-format download. See ADR-005.
- With "Deterministic AI" on, everything runs on the CPU.

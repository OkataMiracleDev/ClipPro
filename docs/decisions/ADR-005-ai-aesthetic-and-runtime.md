# ADR-005: Aesthetic score from CLIP prompts, and AI runtime choices

- Status: accepted
- Date: 2026-10-30

## Context
ALGORITHMS §8.7 calls for an aesthetic score from "a linear/MLP head on the CLIP embedding". The usual head (LAION's aesthetic predictor) ships as PyTorch weights trained on ViT-L/14 embeddings, not the ViT-B/32 model we bundle. Using it would mean a second, larger CLIP model and a PyTorch-format download that the model manager can't verify the way it does ONNX files.

Building the AI pass (P12) also raised three runtime questions the plan doesn't answer:

1. ONNX Runtime 1.31's DirectML provider crashes (access violation) while creating a session for the 8-bit CLIP image encoder with the default graph optimisations. The other four models load fine.
2. Every new input shape makes the DirectML provider recompile, which takes seconds. The last CLIP batch of each clip was usually short.
3. P12.8 requires that core analysis throughput drop by no more than 15% while AI runs. With both at full speed the drop was about 43%, and reloading the models for every clip cost seconds of CPU that couldn't be paused.

## Decision
- **Aesthetic** = `10 · softmax(100 · [cos(e, "a beautiful, well-composed photograph"), cos(e, "a dull, poorly composed photograph")])[0]`, from the same image embedding as shot type. It is normalised as in §8.7 (`smoothstep(3.5, 7.0, x)`). No extra model and no extra download.
- **DirectML sessions** use basic graph optimisation (Level 1), memory patterns off and sequential execution. On this machine's Intel Iris Xe, image embeddings match the CPU run at cosine 0.996 and text embeddings at 0.9997.
- **CLIP batches** always contain 8 images. A short batch is padded with its last image and the extra outputs are dropped.
- **The AI queue yields.** Before each frame and before each audio stage, it waits while any analysis job is queued or running. Loaded models are kept in the session between clips and searches. ONNX Runtime pool threads don't spin when idle, and the queue thread runs at below-normal priority in the app.

## Consequences
- The aesthetic score is coarser than a trained head. It ranks a sharp, well-lit frame above a flat or murky one, but it is not a judgement of composition. The Aesthetic sub-metric is off by default and has low weight in the presets that use it.
- The GPU runs CLIP slower than it could with full optimisation, but it runs. Deterministic AI (CPU only) is unaffected.
- `throughput_with_ai_running` (an ignored test, run on demand) measures a 3–6% drop over about 30 corpus clips with GPU AI, and about 0% with CPU AI. The remaining cost is the one batch already in flight when analysis starts.
- If a later ONNX Runtime fixes the DirectML crash, raise the optimisation level and rerun `directml_detects_and_embeds`.

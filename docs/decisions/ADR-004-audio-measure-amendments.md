# ADR-004: Audio measure amendments

- Status: accepted
- Date: 2026-10-30

## Context
Implementing ALGORITHMS §4 against the `audio_mix` corpus clip (AAC-encoded, like most camera audio) showed four places where the text as written misses real material:

1. **Clipping.** Clipped audio that has been through a lossy codec no longer reaches full scale. The clipped tone decodes with peaks at −0.43 dBFS, so "≥ 3 consecutive samples with |x| ≥ 0.999" never fires.
2. **Silence boundary.** Silence was defined on momentary loudness, which uses a 400 ms window. The flag starts about 400 ms after the audio stops, which misses the ±50 ms event-time test (PLAN P10.1).
3. **Wind.** With the original curves the score stayed between 0.15 and 0.3 on the synthetic gusting wind, so the flag never reached 0.5.
4. **Hum.** A 4096-point FFT has 11.7 Hz bins, too coarse to compare a 50 Hz harmonic with its ±4–20 Hz neighbourhood.

## Decision
- **Clip events.** They also count a *flat-top* block on any channel: block peak ≥ −1 dBFS, with more than 35 % of the samples within 0.5 dB of that peak. A pure sine has about 21 % of its samples within 0.5 dB of the crest; the corpus clipped tone has more than 50 %.
- **Silence.** It uses the K-weighted loudness of the 100 ms block itself (`lufs_b`), below −50 LUFS for ≥ 1 s. Momentary and short-term loudness are kept for scoring and integrated loudness.
- **Wind score.** `smoothstep(0.3, 0.55, lf_ratio) · smoothstep(0.1, 0.35, mean+std of normalised LF flux over 1 s) · (1 − hum)`, with `rms > −45 dBFS`. These are calibration values marked (cal), to revisit with field recordings.
- **Hum.** It is computed from a 1 s window (64k FFT, 0.73 Hz bins) every 500 ms. The window is centred on the block it scores, so it doesn't lag events.

## Consequences
- `audio_mix` matches its truth in every interval. Silence starts within one block of 3.0 s, and there are no false flags on the tone, wind or speech sections.
- EBU Tech 3341 cases 1–2 still read −23.0 and −33.0 LUFS (±0.1) through our own gating of the stored blocks.
- ALGORITHMS §4 is updated to match.

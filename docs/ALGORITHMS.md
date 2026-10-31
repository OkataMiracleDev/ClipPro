# ClipPro: Measurement, Scoring & Segmentation Algorithms

Companion to [SPEC.md](SPEC.md). This document is **normative**: the CPU and GPU kernels and the scoring engine must implement exactly what is written here. Constants marked **(cal)** are initial values that live in `calibration/calibration.toml`; they are tuned in PLAN task P2.9 without code changes. Any change to the maths in this document bumps `engine_version`. Any change to a (cal) value bumps `calibration_version`.

Notation:
- `L(x,y)` is normalised luma in [0,1] on the working image.
- `R` is the sample rate in Hz.
- `fps` is the source frame rate.
- `smoothstep(a,b,x)` is the Hermite step: 0 for x≤a, 1 for x≥b.
- `P_q{…}` is the q-th percentile, using linear interpolation.
- `MAD` is the median absolute deviation.
- Windows are measured in samples, truncated at clip ends and at hard cuts (§2.1).

---

## 0. Sampling plan

### 0.1 Sample times
For a clip whose first presentation timestamp is `t0` and duration is `D`:
- Rate presets: Draft 1, Fast 2, Standard 4, Detailed 8, Thorough 16, All (= every frame).
- `t_k = t0 + k / R` for `k = 0 … floor(D·R)`. The last frame (at `t0 + D − 1/fps`) is always added if it isn't already within `0.5/fps` of the last `t_k`.
- The frame used for sample `k` is the first decoded frame with `pts ≥ t_k`.
- Sample `k` **represents** the interval `[t_k − 0.5/R, t_k + 0.5/R)`, clamped to the clip.

### 0.2 Pair sampling (default on)
For each sample, ClipPro also decodes the **next** frame (`pts` = sample pts + 1/fps). Motion, blur, duplicate and flicker kernels use this pair to get instantaneous (frame-interval) values. In "All" mode the pair frame is simply the next sample, so no extra decode is needed.

### 0.3 Section splitting
Clips longer than `split_threshold` (default 600 s) are divided at keyframes into sections of about 300 s each. Each section except the first starts **one sample earlier** than its nominal range, so the "previous sample" motion for its first real sample can be computed. That overlap sample is discarded when stitching. The temporal pass (§2) runs only after stitching, so section seams have no effect on temporal measures.

## 1. Per-sample measures (kernels)

### 1.0 Working image and active area
1. Decode, then take the luma plane and normalise it to [0,1] (SPEC §9.3). Integer data is converted to f32.
2. Downscale with an **area (box) filter** so the long edge is `≤ 960` px. The aspect ratio is preserved and the image is never upscaled. Output dimensions are rounded to even numbers. This gives `W × H`.
3. **Active area** (letterbox/pillarbox), per clip:
   - Take the first `min(16, N)` samples.
   - For each row compute the mean `m_r` and std `s_r`. A row is "bar" if `median_over_samples(m_r) < 0.035` and `median(s_r) < 0.012`. Do the same for columns.
   - The active rectangle is the largest contiguous non-bar rectangle. It is accepted only if each bar is ≥ 2% of the dimension. Otherwise use the full frame.
   - The check runs again after each hard cut, using the 8 samples that follow it.
   - All later measures use only the active rectangle `A`.
4. **Tile grid** over `A`: `cols = 8`, `rows = clamp(round(8 · H_A / W_A), 4, 8)`. Tiles have equal size, with the remainder spread across them.
5. **Chroma thumbnail**: a 32×18 box-downscaled Cb/Cr thumbnail of `A` (normalised to [0,1], neutral = 0.5) is computed from the decoder's chroma planes. It is used only by §1.4 and §1.2.7.
6. **Luma thumbnail**: a 64×36 box-downscaled thumbnail of `A`, kept for duplicates, cuts and pHash.
7. **Motion pyramid**: `A` is downscaled to a long edge of 320 px (`W_m × H_m`), then halved to 160 and 80 px. This is built for the current sample frame and its pair frame. The previous sample's level-0 pyramid is kept from the previous iteration.

### 1.1 Noise estimate (needed by focus and exposure)
Immerkær fast noise estimate over `A`, excluding strong edges:

```
N = [[ 1,-2, 1],
     [-2, 4,-2],
     [ 1,-2, 1]]
E = { (x,y) ∈ interior(A) : |Sobel(L)| ≤ P90{|Sobel(L)|} }
σ_n = sqrt(π/2) · (1 / (6·|E|)) · Σ_{E} |(L ⊛ N)(x,y)|
```
Stored measure: `noise_sigma = σ_n`.

### 1.2 Focus measures
**1.2.1 Laplacian energy per tile.** Uses `K = [[0,1,0],[1,-4,1],[0,1,0]]`.
`LV_t = Var_{p∈tile}( (L ⊛ K)(p) )`.
White noise of std `σ_n` adds `20·σ_n²` (the sum of K²), so the noise-corrected value is:
`LV'_t = max(0, LV_t − 20·σ_n²)`.

**1.2.2 Tenengrad per tile** (Sobel `Gx`, `Gy`; noise contributes `24·σ_n²`):
`TG'_t = max(0, mean_{tile}(Gx² + Gy²) − 24·σ_n²)`. This is stored for diagnostics and for the focus overlay; Sharpness uses `LV'`.

**1.2.3 Sharpness measures (two variants are stored, blended at score time):**
- `sharp_full = log10(1e-7 + P90_t{ LV'_t })`
- `sharp_centre = log10(1e-7 + P90_w{ LV'_t ; w_t })`. This is a weighted percentile with `w_t = exp(−d_t² / (2·0.30²))`, where `d_t` is the tile centre's distance from the frame centre, normalised so the corner = 1.

The 90th percentile is used deliberately. A sharp subject against a defocused background should score as sharp.

**1.2.4 Detail measure.**
- `τ = max(0.02, 3·σ_n)`
- `edge_density = |{p ∈ A : sqrt(Gx²+Gy²) > τ}| / |A|`
- `tile_coverage = |{t : LV'_t ≥ 0.25 · P90{LV'}}| / tiles`
- `detail_raw = 0.5 · smoothstep(0.01, 0.20, edge_density) + 0.5 · tile_coverage`

**1.2.5 Per-tile values for the overlay.** `LV'_t` is stored as log10, quantised, one u16 per tile. This is required by the focus overlay (VIE-2) and the frame inspector.

### 1.3 Exposure measures
Computed over `A` on full-precision L (10-bit sources keep their precision; histogram with 1024 bins).

| Measure | Definition |
|---|---|
| `mean`, `std` | mean and population std of L |
| `p01, p05, p50, p95, p99` | percentiles from the 1024-bin histogram |
| `clip_black` | fraction of pixels with `L ≤ 0.004` |
| `clip_white` | fraction of pixels with `L ≥ 0.996` |
| `near_black`, `near_white` | fraction with `L ≤ 0.02` / `L ≥ 0.95` |
| `row_band` | banding score, §1.3.1 |
| `pair_dmean` | `|m_t − m_pair| / max(0.02, m_t)`, where the means are taken over the overlap after compensating the pair's measured translation (only with pair sampling) |
| `hist64` | 64-bin normalised histogram (for cuts), stored as u8 per bin |

**1.3.1 Row banding (rolling-shutter flicker).**
With pair sampling the signal is the row-mean *difference* between the sample and its pair frame, after shifting the pair's rows by the measured vertical motion: rolling bands move between frames, while static scene structure cancels out. Without a pair, the frame's own row means are used.
- `r_y` = the mean of row `y` within `A`.
- Detrend: `e_y = r_y − movavg_{H_A/8}(r)_y`.
- Take a direct DFT of `e` at integer frequencies `f = 1 … H_A/2`, measured in cycles per frame height, giving the magnitude spectrum `|E_f|`.
- `ratio = max_{f∈[2,40]} |E_f| / median_{g∈[f−4, f+4], g≠f} |E_g|`. Banding is a narrow peak; scene structure gives a broad, roughly 1/f spectrum, so each frequency is compared with its neighbours.
- `row_band = (ratio − 1) / (ratio + 9)`, multiplied by `smoothstep(0.002, 0.01, std(e))`. The multiplier makes plain gradients score zero.

**1.3.2 White-balance cast (PLUS, informative).** From the chroma thumbnail, using pixels whose luma-thumbnail value is in [0.2, 0.8]:
`wb_cast = sqrt((mean(Cb)−0.5)² + (mean(Cr)−0.5)²)`.

### 1.4 Motion measures
**1.4.1 Block matching.**
- Grid of `16 × 9` blocks (or `9 × 16` when portrait) of 16×16 px, placed uniformly over the level-0 (320 px) image and clamped inside.
- Before matching, each block is zero-meaned and divided by `(std + 0.01)`, which makes matching robust to lighting changes.
- A block is **textured** if its pre-normalisation std ≥ `max(0.015, 2·σ_n)`. Only textured blocks are matched.
- Coarse-to-fine: a global translation search at level 2 (80 px, ±16 px around the prior); then per block, an 8×8 search at level 2 (±6 px), level 1 (±3 px) and a 16×16 refinement at level 0 (±2 px) around the upscaled estimate.
- Sub-pixel refinement: an equiangular (V) fit to SAD at the best offset ±1 on each axis. SAD
  grows linearly away from the match; a parabola there biases offsets toward whole pixels and
  read slow pans (about 3 px per frame) 10–15% slow.
- Output: displacement vectors `v_b`, block centres `p_b` (relative to the image centre) and matching cost `c_b`.

This runs for two frame pairs:
- **Pair motion** (sample → pair frame, Δt = 1/fps). Only when pair sampling is on.
- **Sample motion** (previous sample → current sample, Δt = actual pts difference).

**1.4.2 Global model (similarity transform).** Model: `x' = a·x − b·y + tx`, `y' = b·x + a·y + ty`, with the displacement `v(p) = (x'−x, y'−y)`.
- **RANSAC**: 200 iterations, minimal set of 2 blocks, inlier threshold 1.0 px (level-0 px). The RNG is `xoshiro256**` seeded with `hash(media_fingerprint, sample_index)`, which keeps it deterministic.
- **Refinement**: IRLS with Huber δ = 1.0 px for 5 iterations on the inliers.
- **Outputs**:
  - `tx, ty` (px)
  - `scale = sqrt(a² + b²)`
  - `rot = atan2(b, a)` (rad)
  - `inlier_ratio ρ = inliers / textured`
  - `residual_rms` (px over inliers)
  - `textured_frac = textured / total blocks`
- **Confidence**: `conf = ρ · smoothstep(0.10, 0.40, textured_frac)`. If there are fewer than 6 textured blocks, set `conf = 0` and use the identity model.

**1.4.3 Normalised camera motion (per second, frame-relative).** Let `Δt` be the pair's frame interval. With `W_m` = level-0 width:
- `u = (tx, ty) / W_m / Δt`: translation velocity in **frame widths per second**.
- `zr = ln(scale) / Δt`: zoom rate (1/s, positive = zoom in, meaning the image enlarges).
- `wr = rot / Δt`: rotation rate (rad/s, positive = counter-clockwise in screen space after y-flip, §1.4.4).
- `M_cam = sqrt(|u|² + (0.5·zr)² + (0.5·wr)²)`.

The values are stored as `cam_u_x, cam_u_y, cam_zr, cam_wr, cam_mag, cam_conf`.
- From **pair motion** when available (instantaneous).
- From **sample motion** otherwise (averaged over the sample interval).
- The sample-motion versions are stored separately as `avg_u_x, avg_u_y` for the shake measure.

**1.4.4 Camera direction.** The image moves opposite to the camera. With y-up screen coordinates, the camera pan/tilt vector is `d = (−u_x, +u_y)` (image y points down). Then `cam_dir = atan2(d_y, d_x)` in degrees, in [0,360): 0 = pan right, 90 = tilt up, 180 = pan left, 270 = tilt down.

**1.4.5 Subject motion.** Residuals of textured blocks against the global model: `r_b = |v_b − v_model(p_b)| / W_m / Δt` (frame widths per second).
- `moving_frac` = the fraction of textured blocks with `|v_b − v_model(p_b)| > 1.5 px`.
- `subj_raw = moving_frac · median_{moving}(r_b)`. This is 0 if no blocks are moving.
- Uses sample motion when available (its larger displacements make slow subjects measurable), otherwise pair motion.

**1.4.6 Shake measures** (temporal ones are in §2.4).
- **Instant/average discrepancy**, only with pair sampling: `shake_ia = max(0, |u_pair − u_avg| − 0.05·|u_pair|)`, where `u_avg` is the sample-motion velocity. The 5% allowance covers relative estimation error. A smooth pan gives roughly 0; handheld shake gives large values.
- Rotation and zoom jitter terms are folded into the same measure as `0.2·(max(0, |wr_pair − wr_avg| − 0.05) + max(0, |zr_pair − zr_avg| − 0.05))`. Single-frame zoom and rotation estimates are noisy, so only clear deviations count.
- The sample-interval estimate's coarse search is centred on the pair translation scaled to the sample interval (when the pair confidence > 0.3), so fast pans stay within range.

**1.4.7 Directional blur ratio.**
- Structure tensor over `A` (Sobel gradients, sum over pixels): `J = Σ [Gx², GxGy; GxGy, Gy²]`.
- Let `ê` be the unit image-motion direction (from `u`; undefined if `|u|` < 0.01).
- `E_par = êᵀ J ê`, `E_perp = ê⊥ᵀ J ê⊥`.
- `blur_dir_ratio = E_par / (E_perp + 1e-9)`. Motion blur suppresses gradients along the motion direction, giving a ratio below 1.
- Also stored: `px_per_frame = |u| · W · (1/fps)`, the motion in working-image (960 px) pixels per frame interval.

### 1.5 Similarity measures (for duplicates and cuts)
- `mad_pair` = mean |L_t − L_pair| over the full active area (only with pair sampling). Thumbnails aren't used here because they average sensor noise away, which would make distinct static frames look identical.
- `mad_prev` = mean |thumb_t − thumb_{t−1}|.
- `chi2_prev` = χ² distance between `hist64_t` and `hist64_{t−1}`: `½ Σ (h−g)² / (h+g+1e-9)`.
- `phash64` = 64-bit DCT pHash of the luma thumbnail (resized to 32×32, 8×8 low-frequency DCT block, median threshold).

### 1.6 Pattern detectors (per sample)
- **Solid colour**: `std(L) < 0.010` and `max(std(Cb_thumb), std(Cr_thumb)) < 0.010`. Sub-type: black if `mean < 0.05`, white if `mean > 0.95`, otherwise "colour".
- **Colour bars**: divide the chroma and luma thumbs into 7 equal vertical bands over the top 2/3. Each band's internal std must be < 0.03 and the inter-band luma ordering must match the SMPTE/EBU descending pattern with Pearson r ≥ 0.9 against the template. Templates are stored for SMPTE RP 219 and EBU 100/75.
- **Horizon tilt**: Hough transform on Canny edges of the level-0 image, restricted to lines within ±15° of horizontal. `tilt_deg` = the angle of the strongest accumulator peak if its votes ≥ 0.45·W_m (long, frame-spanning lines only), otherwise NaN.

## 2. Temporal pass (per clip, after all samples are measured)

### 2.1 Windows and boundaries
- `w(s)` is the number of samples spanning `s` seconds: `max(1, round(s·R))`.
- A window around sample `k` covers `[k − w, k + w]`. It is truncated at the clip's ends and **at hard cuts** (§2.6 is computed first), so the window never includes samples from the other side of a cut.
- Medians use exact selection (no approximations), which keeps results deterministic.

### 2.2 Focus stability and hunting
- `S_k = sharp_full_k` (log10 units, "decades").
- `fs_mad_k = MAD_{w(0.75)}(S)`.
- `fs_osc_k` = the number of sign changes in `ΔS` within the window, counting only steps with `|ΔS| > 0.10`, divided by the window length.
- `focus_stab_raw_k = clamp(1 − fs_mad_k / 0.30 − 1.5 · fs_osc_k, 0, 1)` **(cal: 0.30, 1.5)**.
- **Hunting flag** at `k`: within `w(1.0)`, at least 3 sign changes with `|ΔS| > 0.15` and `max S − min S > 0.40` **(cal)**.

### 2.3 Flicker
- `μ_k = mean_k`. `m_k = μ_k − median_{w(1.5)}(μ)`.
- `F_k = RMS_{w(0.5)}(m) / max(0.02, median_{w(1.5)}(μ))`.
- `trend_k = |μ_{k+1} − μ_{k−1}| / (t_{k+1} − t_{k−1}) / fps / max(0.02, μ_k)`: the per-frame relative change the sample-level trend predicts (a smooth exposure ramp is not flicker).
- `damp_k = 1 − smoothstep(0.05, 0.25, cam_mag_k)`: under fast camera motion, sample-level brightness changes mostly come from new content.
- `flicker_raw_k = max( damp_k · smoothstep(0.005, 0.05, F_k), smoothstep(0.004, 0.04, max(0, pair_dmean_k − trend_k)), smoothstep(0.15, 0.45, row_band_k) )` **(cal)**.
- Without pair sampling, the `pair_dmean` term is 0.

### 2.4 Motion stability (shake)
- `u_k` = the stored camera velocity. `lap_k = |u_k − ½(u_{k−1} + u_{k+1})|`, the deviation from the neighbours' mean. It is zero for smooth moves, ramps and single speed steps, and large for jitter. It is NaN at clip ends and next to cuts.
- `J1_k = median_{w(0.5)}(lap)` (a median, so an isolated acceleration doesn't read as shake).
- `J2_k = median_{w(0.5)}(shake_ia)` (0 without pair sampling).
- `J_k = max(J1_k, J2_k)`.
- `stab_raw_k = exp(−J_k / 0.08)` **(cal)** (1 = perfectly stable).
- `stab_conf_k = median_{w(0.5)}(cam_conf)`. Without pair sampling, multiply it by `min(1, R/8)`, because low rates alias shake.
- **Shaky flag** at `k`: `stab_raw < 0.45` and `stab_conf ≥ 0.5`, sustained for ≥ `w(0.5)` consecutive samples **(cal)**.

### 2.5 Motion blur flag
- `S_ref_k = P90_{w(3.0)}(S)`.
- Flag at `k` if all of these hold:
  - `px_per_frame_k ≥ 3.0`
  - `S_k < S_ref_k − 0.35`
  - `blur_dir_ratio_k < 0.70`
  - `cam_conf_k ≥ 0.3` **(cal)**

### 2.6 Hard cuts
- `C_k = 0.5·smoothstep(0.05, 0.6, chi2_prev_k) + 0.5·smoothstep(0.02, 0.2, mad_prev_k)`.
- A cut is at `k` (between samples k−1 and k) if all of these hold:
  - `C_k > 0.25`
  - `C_k > 3 · median_{±5 samples, excluding k}(C)`
  - the sample-motion `conf_k < 0.3` (a fast pan keeps good alignment, a cut doesn't)
  - `mad_pair_k` (if available) `< 0.5·mad_prev_k`, which confirms the new shot is internally consistent.
- Hard cuts are computed **before** the other temporal measures, because windows stop at cuts.

### 2.7 Duplicate flag
- With pair sampling: `mad_pair < 0.0002` and `mad_prev < 0.004`. `mad_pair` is full-resolution, so frozen frames measure about 0 while real static footage keeps measurable sensor noise. A frozen or held frame has near-zero inter-frame difference, below even sensor noise.
- Without pair sampling: `mad_prev < 0.0002`. **(cal)**

### 2.8 Clipping flag
`clip_black ≥ 0.015` or `clip_white ≥ 0.015`. The flag detail records which.

### 2.9 Low-light flag (evaluated at score time, see §6.4)
It depends on the clip's log profile, so it is computed in the scoring engine from stored percentiles.
- `p50 < θ50(profile)` and `p95 < θ95(profile)`, and either `σ_n / max(p50, 0.01) > 0.15` or `p50 < θ50(profile)/2`.
- Defaults for Rec.709/unknown: `θ50 = 0.08`, `θ95 = 0.25`.
- For log profiles: `θ50 = 0.20`, `θ95 = 0.38` **(cal per profile)**.

### 2.10 Colour bars, solid colour and horizon flags
- Colour bars and solid colour flags are taken directly from §1.6.
- Horizon tilt flag: `|median_{w(1.0)}(tilt_deg)| > 2.0°` with a valid angle in ≥ 70% of the window.

### 2.11 Audio-derived flags
Audio-derived flags (§8) are aligned to sample intervals: a sample is flagged if the audio flag covers ≥ 30% of its interval.

## 3. Normalisation to sub-metric values `n ∈ [0,1]`

"Direction" is what n = 1 means. Preferences (§6.1) decide whether high or low n is rewarded.

| Group | Sub-metric | Raw | Mapping → n | Initial constants (cal) | Default state |
|---|---|---|---|---|---|
| Focus | Sharpness (n=1 sharp) | `S_c = lerp(sharp_full, sharp_centre, centre_weighting)` | `logistic((S_c − s0)/k)` | s0 = −3.6, k = 0.35 | on, +0.7 |
| Focus | Detail (n=1 detailed) | `detail_raw` | identity | — | on, +0.3 |
| Focus | Focus Stability (n=1 locked) | `focus_stab_raw` | identity | — | on, +0.5 |
| Exposure | Grading Flexibility (n=1 lots of latitude) | see below | — | — | on, +0.5 |
| Exposure | Flicker (n=1 strong flicker) | `flicker_raw` | identity | — | on, −0.5 |
| Exposure | Luma (target) | `p50` (log profiles: `p50` after profile→709 display transform LUT applied to the value) | target Gaussian | target 0.45, tol 0.15 | off |
| Exposure | Contrast (target) | `p95 − p05` | `clamp(x/0.9,0,1)`, target Gaussian | target 0.55, tol 0.20 | off |
| Exposure | Noise (n=1 noisy) | `σ_n` | `smoothstep(0.003, 0.03, σ_n)` | — | on, −0.2 |
| Exposure | WB cast (n=1 strong cast) | `wb_cast` | `smoothstep(0.01, 0.08, x)` | — | off |
| Motion | Camera Motion (n=1 lots) | `cam_mag` | `1 − exp(−M/0.25)` | 0.25 fw/s | on, 0 (neutral) |
| Motion | Subject Motion (n=1 lots) | `subj_raw` | `1 − exp(−x/0.10)` | 0.10 fw/s | on, 0 |
| Motion | Motion Stability (n=1 stable) | `stab_raw` | identity | — | on, +0.6 |
| Audio | Loudness (target, LUFS) | short-term loudness at sample | target Gaussian in LU | target −23, tol 8 | on (when audio present) |
| Audio | Clean (n=1 no clipping) | clip events/s in ±0.5 s | `exp(−x/0.5)` | — | on, +0.6 |
| Audio | Noise floor (n=1 noisy) | noise floor dBFS | `smoothstep(−70, −35, x)` | — | on, −0.3 |
| Audio | Speech presence (n=1 speech) | VAD fraction in interval | identity | — | on, 0 |
| Audio | Wind (n=1 windy) | wind score | identity | — | on, −0.5 |
| Audio | Hum (n=1 hum) | hum score | identity | — | on, −0.3 |
| Content (AI) | Face in focus, Eyes open, Face size, Aesthetic, Shot-type match | §8.5–8.7 | identity | — | off |

**Grading Flexibility:**
```
h_white = 1 − smoothstep(0.002, 0.05, clip_white)
h_black = 1 − smoothstep(0.005, 0.10, clip_black)
spread  = smoothstep(a, b, p99 − p01)        # Rec.709: a=0.15, b=0.55; log profiles: a=0.10, b=0.40  (cal)
mid     = 1 − smoothstep(0.25, 0.45, |p50 − 0.45|)   # penalise extreme mean exposure
GF      = h_white · h_black^0.7 · (0.5 + 0.5·spread) · (0.6 + 0.4·mid)
```
GF is computed at score time from stored measures so the profile can change without re-analysis.

**Target-type sub-metrics:** `s = exp(−½ · ((x − target)/tol)²)`.

## 4. Audio measures (`clippro-audio`)

Decode to 48 kHz f32 per channel. Make a mono mix as the mean of channels, for features that need one. Process 100 ms blocks (4800 samples) with a 50% overlap for spectral features (FFT 4096, Hann).

| Measure | Definition |
|---|---|
| `peak_dbfs` | max |x| per block, in dBFS |
| `rms_dbfs` | per block |
| `true_peak_dbtp` | 4× polyphase oversampled peak (ITU-R BS.1770-4 Annex 2) |
| `lufs_m`, `lufs_s` | momentary (400 ms) and short-term (3 s) loudness per BS.1770 via `ebur128` |
| `clip_events` | count of runs of ≥ 3 consecutive samples with \|x\| ≥ 0.999, plus one per flat-top block (peak ≥ −1 dBFS and > 35 % of samples within 0.5 dB of it; catches clipping after lossy coding, ADR-004) |
| `lf_ratio` | energy < 150 Hz / total energy (20 Hz–20 kHz) |
| `flatness` | spectral flatness (geometric/arithmetic mean of power, 100 Hz–8 kHz) |
| `flux_lf` | positive spectral flux in 20–150 Hz |
| `hum_score` | for f0 ∈ {50, 60}: mean over harmonics h=1..5 of `smoothstep(6, 18, P(h·f0) − median P(h·f0 ± 4..20 Hz))` in dB; take the max over f0. Computed on a 1 s window (64k FFT) every 500 ms, centred on the block (ADR-004) |
| `lufs_b` | K-weighted loudness of the 100 ms block itself (silence boundaries) |
| `speech_band` | energy 300–3400 Hz / total |
| `zcr` | zero-crossing rate |

**Derived (per 100 ms block, then aggregated to sample intervals):**
- **Noise floor**: `P10` of `rms_dbfs` over a ±5 s window.
- **VAD**: a block is speech if all of these hold: `rms_dbfs > noise_floor + 6`, `flatness < 0.5`, `speech_band > 0.5`, and `zcr ∈ [0.02, 0.35]`. Then apply a 300 ms hangover and a 3-of-5 majority smoothing.
- **Wind score**: `smoothstep(0.3, 0.55, lf_ratio) · smoothstep(0.1, 0.35, mean + std of normalised flux_lf over 1 s) · (1 − hum_score)`, with `rms_dbfs > −45` **(cal, ADR-004)**.
- **Flags**:
  - Audio clipping: ≥ 1 clip event, or `true_peak > −0.1 dBTP` in the interval.
  - Silence: `lufs_b < −50` for ≥ 1 s (ADR-004).
  - Wind: wind score > 0.5 for ≥ 0.5 s.
  - Hum: `hum_score > 0.5` for ≥ 2 s.
  - Dead channel: channel RMS < −80 dBFS over the whole clip while another channel is > −60 dBFS.
  - Channel imbalance: |RMS_L − RMS_R| > 10 dB sustained for ≥ 5 s, on pairs declared stereo.
- **Segment integrated loudness**: computed at score time from the stored 100 ms gating blocks (BS.1770 two-stage gating), shown in the inspector and in reports.
- **Waveform peaks**: min/max pyramid at 256, 2048 and 16384 samples per bin, for each channel.

## 5. Determinism rules
1. **CPU kernels**: reductions use fixed-order pairwise summation in f64 per tile/row, then convert to f32. No `par_iter().sum()` over floats without a fixed chunking.
2. **GPU kernels**: per-tile and per-block reductions use groupshared memory with a fixed tree order. Histograms use integer atomics (order-independent). Cross-group float sums are written per group to a buffer, then summed on the CPU in a fixed order.
3. **Quantisation**: before storage, each measure is quantised to u16 over its declared range `[lo, hi]` (table in `calibration.toml` `[ranges]`). Parity is defined on the quantised values.
4. **RANSAC RNG** is seeded per sample (§1.4.2).
5. **Reproducible mode**: CPU kernels only, a deterministic sample order, and the same results on any x86-64-v2 machine. SIMD paths must produce bit-identical results to scalar, or be disabled in this mode.

## 6. Scoring

### 6.1 Sub-metric scores (stage S1)
Each sub-metric `i` in group `g` has either:
- a **bipolar preference** `p_i ∈ [−1, 1]` (0 or off means excluded):
  - `s_i = n_i` if `p_i > 0`, otherwise `s_i = 1 − n_i`
  - weight `ω_i = |p_i|`
- or a **target**: `s_i = exp(−½((x−target)/tol)²)`, with weight `ω_i = strength` (default 1).

**Confidence**: for motion sub-metrics, `s_i ← conf·s_i + (1−conf)·0.5` (unsure means neutral). `conf` is `cam_conf` for Camera/Subject motion and `stab_conf` for Motion Stability.

### 6.2 Group scores (stage S2)
`G_g = Σ_i ω_i s_i / Σ_i ω_i`. If `Σ ω_i = 0` the group is **unavailable** (the UI shows an amber label). A group is also unavailable for a clip that lacks its data (e.g. no audio stream, or the group wasn't analysed).

### 6.3 Overall (stage S3)
`O = Σ_g W_g G_g / Σ_g W_g`, over available groups with `W_g > 0`. If no group is available, `O = 0` and the clip shows "No scoring groups".
- **Stability emphasis** `e ∈ [0,1]`: `O ← O · (1 − 0.5·e·(1 − n_stab))` when Motion data exists.
- **Centre weighting** `c ∈ [0,1]` is applied in Sharpness (§3), not here.

### 6.4 Modifiers (stage S4)
1. Evaluate the score-time flags (Low light, §2.9) and merge them with the stored flags.
2. If the **Flags** master switch is off, skip steps 3–4 (flags become informational).
3. Tiers per sample: `m = Π_{flags present} f(tier)`, where Disfavour = 0.6, Favour = 1.2 and Neutral = 1.0.
   - `force_exclude_k` = any flag present with tier Force exclude.
   - `force_include_k` = any flag present with tier Force include.
4. **Precedence**: manual override > flag Force include > flag Force exclude > score rules. A sample with both flag-forces counts as force-include.
5. **Camera filter** factor `cm` (§6.5), which is 1 if the filter is off.
6. If the **Metrics** master switch is off, `O = 1` for every sample (selection then depends only on flags and the camera filter).
7. `F_k = clamp(O_k · m_k · cm_k, 0, 1)`.

### 6.5 Camera-movement filter
UI state:
- puck `(r ∈ [0,1], φ0)`
- zoom slider `zs ∈ [−1,1]`
- rotate slider `rs ∈ [−1,1]`
- `static_only`, `mirror`

Per sample, with `d`, `cam_dir`, `zr`, `wr` from §1.4:
- **Static only**: `cm = exp(−cam_mag / 0.03)`. The puck and sliders are ignored.
- **Pan/tilt** (active if `r ≥ 0.05`):
  - `A = lerp(90°, 5°, r)` (half-angle).
  - `δ` = angular distance between `cam_dir` and `φ0`. With `mirror`, use `min(δ, angular distance to φ0+180°)`.
  - `dm = 1 − smoothstep(0.5·A, A, δ)`.
  - `mm = smoothstep(0.02, 0.10, |u|)`, so movement is required.
  - `c_pt = dm · mm`.
- **Zoom** (active if `|zs| ≥ 0.05`):
  - `zmin = lerp(0.02, 0.30, |zs|)`.
  - `c_z = smoothstep(0, zmin, sign(zs)·zr)`. With `mirror`: `smoothstep(0, zmin, |zr|)`.
- **Rotate** (active if `|rs| ≥ 0.05`):
  - `wmin = lerp(2°, 30°, |rs|)/s` in rad/s.
  - `c_w = smoothstep(0, wmin, sign(rs)·wr)`. With `mirror`: use `|wr|`.
- `c = Π(active components)`, or 1 if none are active.
- `cm = cam_conf·c + (1 − cam_conf)·0.5`.

The filter multiplies the score. It never hard-excludes; the score threshold makes the final decision.

## 7. Segmentation (stages S5–S7)
Inputs per clip:
- final scores `F_k`, force flags, hard-cut positions
- thresholds `[T_lo, T_hi]`, length `[L_min, L_max]` (L_max may be ∞)
- gap tolerance `g` s, handles `H` samples, max per clip `N` (may be ∞)
- `long_segment_mode ∈ {split, trim_best}` (default split)

### S5: Candidate runs
1. `cand_k = (F_k ≥ T_lo ∨ force_include_k) ∧ ¬(force_exclude_k ∧ ¬force_include_k)`.
2. Form maximal runs of `cand`. Merge two adjacent runs if they are separated by ≤ `G = round(g·R)` non-candidate samples **and** the gap contains no force-excluded sample **and** no hard cut.
3. Split runs at hard cuts.

### S6: Segments
For each run:
1. **Span**: `[start_k − 0.5/R, end_k + 0.5/R)`, clamped to the clip.
2. **Mean score** `μ` = mean of `F` over **all** samples in the span, including merged gap samples. Gaps therefore lower the mean, which is intended.
3. **Length rules** (before handles):
   - If `len < L_min`: drop it. Force-include runs are dropped too; this is consistent with "segment-length limits still apply".
   - If `len > L_max`:
     - `trim_best`: keep the window of length `L_max` with the highest mean (O(n) sliding window; ties go to the earliest).
     - `split`: take the best window as above, then process the left and right remainders recursively as runs (each must still meet `L_min`).
4. **Score range**: keep if `T_lo ≤ μ ≤ T_hi`, or if the segment contains any force-include sample.
5. **Handles**: extend each side by `H/R` seconds.
   - Never beyond the clip bounds or across a hard cut.
   - If two segments' handles would overlap, split the gap at its midpoint.
   - Handles don't change `μ` or `len` for the rules above. They are included in exports.

### S7: Cap, overrides, sort
1. **Per-clip cap**: rank non-forced segments by `μ` descending, then by `len` descending, then by start ascending. Keep the top `N`. Force-include segments are always kept and don't count toward `N`.
2. **Manual overrides**, anchored to time ranges so they survive re-scoring:
   - `force_out [a,b)`: remove any segment overlapping the range by ≥ 50% of the segment's length.
   - `force_in [a,b)`: add a segment `[a,b)`, merged with overlapping segments. Its `μ` is computed over its span.
   - `trim [a,b)` linked to the nearest segment: replace that segment's edges.
   - `manual_segment`: same as force_in, but shown with a distinct style.
3. **Sort** for display or export: timeline (clip order, then start) | μ↓ | μ↑ | len↓ | len↑. Sorting never changes membership.
4. **Summary**: count, total duration (excluding handles), % of analysed footage, per-flag run counts (runs of consecutive flagged samples across all clips).

### Complexity and performance
- Every stage is O(samples), using SoA `f32` arrays per clip and processing clips in parallel with rayon.
- Stage outputs are cached per clip, keyed by a hash of the relevant settings (SPEC §11).
- The target is 1.44 M samples (100 h at 4 Hz) through S1–S7 in ≤ 150 ms on 6 cores. That needs no allocation in the hot loops, precomputed `n` columns, and flag bitsets.

## 8. PLUS algorithms

### 8.1 Sentence snapping (needs Transcripts)
- **Boundaries** = sentence ends (punctuation from Whisper), plus pauses where the gap between words is > 0.35 s.
- For each segment edge, move it to the nearest boundary within ±1.5 s. A start edge moves to a word start; an end edge moves to a word end plus 0.1 s.
- Snapping never moves an edge into a force-excluded sample or across a hard cut. If no boundary is within range, the edge stays put.
- Snapping runs after S6 and before handles.

### 8.2 Flub and filler detection
- **Fillers**: an n-gram list per language (en: "um", "uh", "erm", "ah", "you know", "i mean"), producing a word-level flag.
- **Restart**: the same 2–4-gram repeated within 2.0 s, or a self-correction pattern ("I— I mean"). Produces a transcript flag, "Flub".

### 8.3 Audio sync
1. **Envelope**: half-wave rectified spectral flux of the mono mix at 100 Hz frames (FFT 1024, hop 480 at 48 kHz). Subtract the mean and divide by the std.
2. **Coarse lag**: FFT cross-correlation, zero-padded, over the full overlap range allowed (the user can restrict it, e.g. ±10 min). Peak `ℓ*` with confidence `PSR = (peak − mean_sidelobe) / std_sidelobe`, where sidelobes exclude ±0.5 s. Accept if `PSR ≥ 6`.
3. **Fine lag**: GCC-PHAT on raw audio decimated to 8 kHz, in a 10 s window around the strongest onset region, searching ±50 ms around `ℓ*`. Parabolic interpolation gives sub-sample accuracy.
4. **Group**: the reference is the longest clip. Every other clip is synced to the reference, or failing that to any already-synced clip, chaining with consistency checks (a loop disagreement > 1 frame produces a warning).
5. **Timecode mode**: align by start TC. If both TC and audio are available, report disagreements > 1 frame.

### 8.4 Take grouping
Per-segment descriptor:
- `ph` = pHash of the segment's middle sample.
- `hist` = mean `hist64`.
- `clip` = mean CLIP embedding (if AI is on).
- `txt` = normalised transcript text (if available).

Similarity:
```
sim = 0.5·cos(clip_a, clip_b) + 0.3·J3(txt_a, txt_b) + 0.2·(1 − hamming(ph_a, ph_b)/64)    # full
sim = 0.6·(1 − hamming/64) + 0.4·(1 − χ²(hist))                                          # no AI
```
`J3` is the Jaccard similarity over word 3-grams. Missing terms are dropped and the remaining weights are renormalised.

Grouping and ranking:
- Average-linkage agglomerative clustering with threshold `sim ≥ 0.80` **(cal)**. Only segments from different clips, or more than 30 s apart in the same clip, are considered.
- **Best take** = the highest `μ`. Ties go to fewer flubs, then longer duration. Rank 1 is "best".

### 8.5 Faces
Faces need a colour decode at 2 Hz and 640 px long edge. This is an AI job with its own decode pass.

1. **Detection**: run detection (score ≥ 0.6), then NMS at IoU 0.4.
2. **Alignment**: use the 5-point landmarks to warp to 112×112, then compute embeddings.
3. **Tracks**: greedily link detections across samples by IoU ≥ 0.3 and embedding cosine ≥ 0.5.
4. **People**: per project, agglomerative clustering of track-mean embeddings (cosine distance, threshold 0.45 **(cal)**). The user can merge, split and name people. Named people are stable across re-clustering via the stored embedding centroid.
5. **Per-sample, per-face quality measures**:
   - `face_focus`: the noise-corrected Laplacian variance (§1.2.1) inside the face box on the luma working image, log10, mapped with `logistic((x − (s0 + 0.3))/k)`.
   - `face_size`: box area / frame area.
   - `eyes_open`: needs an eye-state classifier. Only enabled if a permissively licensed model is available (PLAN P12 decides). Otherwise the sub-metric is hidden.
6. **Sample-level Content sub-metrics**:
   - Face in focus = the max `face_focus` over faces (or over the selected People filter).
   - Face size = `smoothstep(0.005, 0.15, face_size)`.
   - "No face" flag when no face is detected (tier default Neutral).

### 8.6 Shot type
- Compute a CLIP image embedding at 2 Hz (shared with semantic search).
- Zero-shot prompts per class: "a wide shot of …", "a medium shot", "a close-up shot of a face", "an extreme close-up", "an insert shot of an object", "an aerial drone shot", "an over-the-shoulder shot". Take the softmax over cosine × 100.
- Fuse with face size: if the largest face is > 12% of the frame, boost close-up by +0.2; if it's > 35%, boost extreme close-up. Renormalise.
- The per-segment class is the mode over its samples, with a confidence.
- The "Shot-type match" sub-metric is the probability of the user-selected class(es).

### 8.7 Aesthetic, semantic search, best thumbnail
- **Aesthetic**: 0–10 from CLIP prompt contrast, "a beautiful, well-composed photograph" against "a dull, poorly composed photograph" (ADR-005, in place of a trained head). Normalised as `smoothstep(3.5, 7.0, x)`.
- **Semantic search**: query text → CLIP text embedding. Cosine against stored image embeddings. Results are aggregated per segment (max) and per clip, and shown ranked with a threshold slider.
- **Best thumbnail** per segment: `argmax_k` of `F_k · (0.5 + 0.5·face_in_focus_k if faces on) · (1 − 0.5·motion_blur_k)` over samples away from the edges (ignoring the outer 10% of the span).

## 9. Built-in presets

Legend:
- Weights are F/E/M/A/C.
- Prefs are listed only where they differ from the defaults in the §3 table.
- Flags are listed only where they differ from these defaults: clipping = disfavour, duplicate = force_exclude, solid = force_exclude, hard cut = neutral, blur = disfavour, shaky = disfavour, low light = disfavour, PLUS flags = neutral.
- Gap is in seconds.

| Preset | Score [lo,hi] | Length [min,max] | Cap | Weights F/E/M/A/C | Notable prefs | Flags | Camera filter | Advanced |
|---|---|---|---|---|---|---|---|---|
| All-Rounder | 0.55, 1.00 | 2s, none | none | 35/30/25/10/0 | defaults | defaults | off | gap 0.75, centre 0.2, stab 0.4 |
| Talking Head | 0.62, 1.00 | 3s, none | none | 40/30/20/10/0 | cam −0.6, subj 0, flicker −0.7, speech +0.5 | shaky disfavour | off | gap 1.0, centre 0.5, stab 0.5 |
| Social Media | 0.60, 1.00 | 1s, 15s | 3 | 35/30/30/5/0 | subj +0.4, cam +0.2 | blur disfavour | off | gap 0.4, centre 0.4, stab 0.3, mode trim_best |
| Cleanup | 0.25, 1.00 | 0.5s, none | none | 34/33/33/0/0 | all prefs halved | solid, duplicate, blur, low light → force_exclude | off | gap 0.25, centre 0, stab 0.2 |
| Action / Sport | 0.50, 1.00 | 1s, 20s | none | 30/20/50/0/0 | cam +0.6, subj +0.8, stab +0.2 | blur neutral, shaky neutral | off | gap 0.5, centre 0.3, stab 0.1 |
| Long Takes | 0.60, 1.00 | 10s, none | none | 35/30/35/0/0 | stab +0.9, focus_stab +0.8 | defaults | off | gap 1.5, centre 0.2, stab 0.8 |
| Drone / Aerial | 0.60, 1.00 | 4s, none | none | 40/30/30/0/0 | sharp +0.9, stab +1.0, cam +0.3, flicker −0.6 | shaky, blur → force_exclude | off | gap 1.0, centre 0, stab 0.9 |
| Whip Pans | 0.45, 1.00 | 0.3s, 3s | none | 10/20/70/0/0 | cam +1.0, stab 0, sharp 0 | blur neutral, shaky neutral | puck φ0=0°, r=0.7, mirror | gap 0.2, stab 0 |
| Locked-Off | 0.60, 1.00 | 3s, none | none | 40/30/30/0/0 | cam −1.0, stab +1.0 | shaky force_exclude | static only | gap 1.0, stab 1.0 |
| Twist | 0.50, 1.00 | 1s, none | none | 30/25/45/0/0 | cam +0.5 | defaults | rotate rs=0.5, mirror | gap 0.5 |
| Tilt | 0.50, 1.00 | 1s, none | none | 30/25/45/0/0 | cam +0.5 | defaults | puck φ0=90°, r=0.7, mirror | gap 0.5 |
| Pan | 0.50, 1.00 | 1s, none | none | 30/25/45/0/0 | cam +0.5 | defaults | puck φ0=0°, r=0.7, mirror | gap 0.5 |
| Interview (PLUS) | 0.62, 1.00 | 3s, none | none | 35/25/15/10/15 | Talking Head + face_focus +0.8, face_size +0.3, speech +0.8 | Flub disfavour, No face disfavour | off | snap_to_sentences on |
| Wedding / Event (PLUS) | 0.50, 1.00 | 2s, 30s | none | 35/35/25/5/0 | flicker −0.8, subj +0.2 | low light neutral | off | gap 0.75 |
| B-roll (PLUS) | 0.58, 1.00 | 3s, 12s | 4 | 40/30/30/0/0 | stab +0.7, cam +0.2 | defaults | off | mode trim_best |
| Documentary Verité (PLUS) | 0.45, 1.00 | 4s, none | none | 30/30/20/20/0 | speech +0.6, stab +0.3 | shaky neutral | off | gap 1.5 |
| Product / Tabletop (PLUS) | 0.65, 1.00 | 2s, none | none | 45/40/15/0/0 | sharp +1.0, detail +0.8, GF +0.8, cam −0.3 | clipping force_exclude | off | centre 0.6 |
| Gimbal Walk (PLUS) | 0.55, 1.00 | 3s, none | none | 35/25/40/0/0 | stab +1.0, cam +0.5 | shaky force_exclude | off | stab 0.9 |
| Audio-First (PLUS) | 0.50, 1.00 | 3s, none | none | 15/15/10/60/0 | speech +1.0, clean +1.0, wind −0.8, hum −0.8, loudness target −16 | Audio clipping disfavour, Silence force_exclude | off | snap_to_sentences on (if transcripts) |

## 10. Calibration procedure (P2.9)
1. **Synthetic sweep**: for each mapping, generate controlled ramps (blur σ 0→6 px at 1080p; exposure −4→+3 stops; noise σ 0→0.05; pan 0→3 fw/s; shake RMS 0→0.2 fw/s). Fit the constants so the documented anchors hold:
   - Sharpness: blur σ 0 → n ≥ 0.85; σ 1.5 → n ∈ [0.35, 0.6]; σ 4 → n ≤ 0.15, at 1080p and 4K sources (the 960 px working image makes them comparable).
   - Motion Stability: tripod → ≥ 0.9; gimbal walk (synthetic smooth sine 0.2 Hz) → ≥ 0.8; handheld jitter RMS 0.1 fw/s → ≤ 0.35.
   - Camera Motion: 0.25 fw/s pan → n ≈ 0.63.
2. **Real-footage check** (human checkpoint): a 30-clip mixed reference set chosen by the owner. The builder produces an HTML calibration report (per clip: score timeline, overlays, thumbnails at decile scores) for review. Adjustments are made only in `calibration.toml`.
3. `clippro-cli calibrate --sweep` runs step 1 and writes a report plus proposed constants. Constants are never auto-applied.

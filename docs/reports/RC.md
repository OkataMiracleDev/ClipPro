# ClipPro 1.0 release-candidate checklist (P14.7)

Status on 2026-10-30, branch `dev`. Each SPEC §5 feature ID is shown with its evidence: an automated test (file and test name), a recorded measurement, or a human check (HC) that only the owner can do.

**Status key:**
- **Pass**: automated evidence passes.
- **Human**: needs the owner's check; the automated part passes.
- **Gap**: not met. The reason is recorded.
- **Later**: moved out of 1.0 by an ADR.

**Test runs at this point:**
- `cargo test --workspace --release`: 225 tests pass.
- `npm --prefix app/ui test`: 63 tests pass.
- `scripts/verify.ps1` passes every step. The on-demand runs are below.

## 5.1 Ingest
| ID | Status | Evidence |
|---|---|---|
| ING-1 | Pass | `core/tests/session.rs::import_analyse_score_reopen_and_cache` (folders, recursion, duplicates, failures); `scripts/e2e.mjs` imports through the app. Drag and drop: manual check only |
| ING-2 | Pass | `media/tests/corpus.rs::probe_every_corpus_clip`, `preview_seek_accuracy_across_codecs` (H.264, HEVC 10-bit, ProRes, DNxHR, 4:2:2 10-bit, VFR MKV) |
| ING-3 | Pass | `core::import::tests::proxy_names`, `session.rs::retry_bump_and_link_proxy`, `import_analyse_score_reopen_and_cache` (proxy analysed, original exported) |
| ING-4 | Pass | `core::watch::tests::slow_copy_is_picked_up_once_when_stable`, `accepts_by_folder_depth_and_extension` |
| ING-5 | Pass | `media/tests/corpus.rs::probe_every_corpus_clip` (TC, reel, camera, date, audio channels) |
| ING-6 | Pass | `ingest::offload::tests::*` (verify, corruption detected, resume, cancel), `mhl::tests::manifest_shape`, `hash::tests::known_vectors` |
| ING-7 | Pass | `ingest/tests/encode.rs::proxy_keeps_timecode_reel_and_length`, `session.rs::proxies_and_rendered_selects` |
| ING-8 | Pass | `session.rs::import_analyse_score_reopen_and_cache` (second project reuses the cache), `project::store::tests::measure_cache_lru` |
| ING-9 | Pass | `session.rs::import_analyse_score_reopen_and_cache` (duplicate copy detected) |

## 5.2 Analysis
| ID | Status | Evidence |
|---|---|---|
| ANA-1 | Pass | `media::video::tests::plan_*`, `media/tests/corpus.rs::sampling_times_and_pairs`, `every_frame_mode_and_vfr` |
| ANA-2 | Pass | `measure::image::tests::*`, `sample::tests::letterbox_detected`, `dark_scene_is_not_letterbox` |
| ANA-3 | Pass | `analysis/tests/corpus_metrics.rs::corpus_expectations`, `measure::spatial::tests::sharpness_decreases_with_blur`, `centre_weighting_rewards_sharp_centre` |
| ANA-4 | Pass | `corpus_expectations` (exposure ramp, flicker), `spatial::tests::exposure_percentiles_on_ramp`, `noise_estimate_tracks_injected_noise` |
| ANA-5 | Pass | `measure::motion::tests::recovers_translation`, `recovers_zoom_and_rotation`, `session.rs::camera_presets_pick_matching_clips` |
| ANA-6 | Pass | `media/tests/corpus.rs::sampling_times_and_pairs` |
| ANA-7 | Pass | `project::store::tests::run_keys_differ_by_settings`, `score::settings::tests::starved_group` |
| ANA-8 | Pass (decode) / Later (GPU measurement) | `media/tests/corpus.rs::hardware_decode_matches_cpu`; GPU measurement deferred by ADR-006 |
| ANA-9 | Pass | `analysis/tests/scheduler.rs::split_sections_equal_single_pass`, `bad_file_fails_alone_and_bump_reorders` |
| ANA-10 | Pass | `scheduler.rs::pause_resume_and_restart_equal_single_pass`, `bad_file_fails_alone_and_bump_reorders`, `project/tests/crash.rs::killed_writer_recovers` |
| ANA-11 | Pass | `audio/tests/corpus.rs::audio_mix_matches_truth`, `audio::tests::ebu_3341_cases_1_and_2`, `flags_from_synthetic_signals` |
| ANA-12 | Pass | `core/tests/reproducible.rs::cpu_measurements_are_bit_identical` (runs, workers, decoder threads, split sections; hardware decode also identical on the dev machine) |

## 5.3 Flags
| ID | Status | Evidence |
|---|---|---|
| FLG-1 to FLG-7 | Pass | `corpus_expectations` against `testdata/truth/*.json`, `spatial::tests::clip_fractions_exact`, `temporal::tests::*` |
| FLG-8 | Pass | `corpus_expectations` (focus hunt, bars, horizon), `sample::tests::horizon_tilt_detects_angle`, `spatial::tests::bars_detector`, `audio_mix_matches_truth` (clipping, silence, wind, hum) |
| FLG-9 | Pass | `score/tests/engine.rs::golden_tier_multipliers`, `force_include_exemptions`, `inv4_force_out_never_selected_without_override` |
| FLG-10 | Pass | `ui: lib/flagRuns.test.ts`, `engine.rs::sort_orders_and_summary` |

## 5.4 Tuning and scoring
| ID | Status | Evidence |
|---|---|---|
| TUN-1 to TUN-3 | Pass | `engine.rs::inv1_*`, `inv2_*`, `length_rules_split_and_trim`, `per_clip_cap_with_ties`; `ui: lib/typed.test.ts` |
| TUN-4, TUN-5 | Pass | `engine.rs::golden_group_weights`, `golden_bipolar_positive_and_negative`, `golden_target_luma`; `ui: lib/submetrics.test.ts` |
| TUN-6 | Pass | `engine.rs::golden_camera_filter`; `ui: views/CameraWheel.test.ts`; `session.rs::camera_presets_pick_matching_clips` |
| TUN-7 | Pass | `engine.rs::golden_metrics_switch_and_stability_emphasis` |
| TUN-8 | Pass | `engine.rs::gap_merging_and_cut_splitting`, `handles_respect_bounds_cuts_and_neighbours` |
| TUN-9 | Pass | `engine.rs::incremental_stages`, `cancellation_returns_none_and_leaves_cache_consistent`; 100 h re-score 108 ms worst case (bench `rescore`); memory soak below |
| TUN-10 | Pass | `ui: lib/history.test.ts`, `lib/diff.test.ts` |
| TUN-11 | Pass | `session.rs::overrides_and_annotations`, `engine.rs::overrides_survive_rescoring`, `core::annotate::tests::*` |
| TUN-12 | Pass | `engine.rs::sentence_snapping`, `transcribe::tests::snapping_and_subtitles`, `core/tests/ai.rs::ai_pass_people_search_and_content` |
| TUN-13 | Pass | `core/tests/ai.rs::takes_group_and_best_take` |

## 5.5 Presets
| ID | Status | Evidence |
|---|---|---|
| PRE-1, PRE-2 | Pass | `score::preset::tests::every_builtin_parses_and_validates`, `settings::tests::talking_head_resolves`, `audio_first_loudness_target`, `session.rs::camera_presets_pick_matching_clips` |
| PRE-3 | Pass | `app::presets::tests::save_list_rename_import_delete`, `preset::tests::unknown_keys_survive_roundtrip` |

## 5.6 Viewer
| ID | Status | Evidence |
|---|---|---|
| VIE-1 | Pass | `media/tests/corpus.rs::preview_seek_accuracy_across_codecs`, `preview_frames_play_forward_without_seeking`, `app::audio_out::tests::audio_clock_tracks_wall_time`; `ui: lib/shuttle.test.ts` |
| VIE-2 to VIE-4 | Pass | `ui: lib/glview.test.ts`, `lib/scope.test.ts`; screenshots in the preview build |
| VIE-5 | Pass | `session.rs::inspect_agrees_with_selects` (1,000 random samples), `engine.rs::explain_matches_fast_path` |
| VIE-6 | Pass | `score::colour::tests::curves_match_vendor_references`, `display_rendering_anchors`; `ui: lib/cube.test.ts` |
| VIE-7 | Pass (visual) | Split and A/B compare checked in the preview build (`?compare=1`); no automated test. The look is part of HC-9 |
| VIE-8 | Pass | `audio::tests::alignment_to_video_samples`, `app::audio_out::tests::*`; per-channel mute and solo in `AudioMeters` |

## 5.7 Timeline
| ID | Status | Evidence |
|---|---|---|
| TML-1 to TML-4 | Pass | `engine.rs::sort_orders_and_summary`; `scripts/e2e.mjs` (P5.7 scenario) |
| TML-5 | Pass | Grouping by clip, camera, date, reel, person, shot type and take group: `core::stringout::tests::groups_by_key_and_by_best`, `groups_by_ai_fields`; person and shot-type smart collections `ui: lib/smart.test.ts` |

## 5.8 Export and interchange
| ID | Status | Evidence |
|---|---|---|
| EXP-1, EXP-2, EXP-4 | Pass / Human | `interchange::fcp7::tests::*`, `fcpxml::tests::*`, `otio::tests::structure`, `import::tests::xml_and_otio_round_trip`, `tests/samples.rs::*`. **HC-3**: open the exports in the owner's NLEs |
| EXP-3 | Pass / Human | `tests/aaf_pyaaf2.rs::pyaaf2_reads_our_aaf`, `reads_a_pyaaf2_authored_file`, `aaf::tests::round_trip_through_our_reader`. **HC-4**: open in Media Composer or Premiere (beta until then) |
| EXP-5 | Pass | `edl::tests::*`, `transcribe::tests::snapping_and_subtitles` (SRT/VTT) |
| EXP-6 | Pass | `model::tests::annotation_tokens_and_label_colour` (incl. `{people}`, `{shot}`, `{transcript}`) |
| EXP-7 | Pass | `model::tests::selects_only_layout`, `selects_over_rejects_covers_whole_clips`, `without_handles_uses_core_span` |
| EXP-8 | Pass | `report::tests::*`, `session.rs::html_report_and_contact_sheet`; PDF via `ClipPro --pdf-check` (also run from the installed copy) |
| EXP-9 | Pass | `ingest/tests/encode.rs::rendered_selects_match_their_length`, `render::tests::names_and_copy_points` |
| EXP-10 | Pass | `session.rs::imported_timeline_review_and_export_back`, `core::review::tests::*`, `tests/samples.rs::*` |
| EXP-11 | Pass | `session.rs::multicam_sync_groups`, `audio::sync::tests::*`, `fcpxml::tests::multicam_angles` |
| EXP-12 | Pass | `model::tests::stringout_groups_and_separators`, `stringout::tests::*` |

## 5.9 Projects
| ID | Status | Evidence |
|---|---|---|
| PRJ-1 | Pass | `session.rs::import_analyse_score_reopen_and_cache`, `project::db::tests::create_reopen_and_markers` |
| PRJ-2 | Pass | `project/tests/crash.rs::killed_writer_recovers`, `db::tests::migrates_v0_with_backup` |
| PRJ-3 | Pass | `interchange::import::tests::relink_*`, `session.rs::retry_bump_and_link_proxy` |
| PRJ-4, PRJ-5 | Pass / manual | Smart collections: `ui: lib/smart.test.ts`. Recent projects: `app::settings::tests::round_trip_sanitise_and_options`. Bins and templates: manual check only |

## 5.10 AI modules
| ID | Status | Evidence |
|---|---|---|
| AI-1 | Pass | `ai::models::tests::manifest_is_complete`; download/verify/resume exercised when installing the models; consent gate in Settings → AI Models |
| AI-2 | Pass / Human / Gap | `ai::faces::tests::real_faces`, `alignment_tracks_and_clusters`, `core/tests/ai.rs::ai_pass_people_search_and_content` (one person, names kept on re-cluster). **HC-6**: spot-check on real footage. **Gap**: eyes-open is hidden; no permissively licensed model (docs/models.md) |
| AI-3 | Pass | `transcribe::tests::whisper_runs`, `fillers_and_flubs`, `ai::speakers::tests::*`, `ai_pass_people_search_and_content` (language, FTS search) |
| AI-4 | Pass | `ai::clip::tests::zero_shot_with_installed_models`, `core::ai::tests::ai_data_lookups`; indoor/outdoor and day/night as a smart rule |
| AI-5 | Pass | `ai_pass_people_search_and_content` (visual search) |
| AI-6 | Pass | Aesthetic sub-metric reaches the engine (`ai_pass_people_search_and_content`); method in ADR-005 |
| AI-7 | Pass | `core::ai::tests::thumbnail_prefers_sharp_faces_away_from_edges` |
| AI-8 | Pass | `core/tests/ai.rs::takes_group_and_best_take` |

## 5.11 Other
| ID | Status | Evidence |
|---|---|---|
| OTH-1 | Pass | `app::settings::tests::round_trip_sanitise_and_options`, `ui: lib/keys.test.ts` |
| OTH-2 | Pass | `ui: lib/markdown.test.tsx` (manual rendering, every shortcut documented). First-run tour: manual check (`?tour=1`) |
| OTH-3 | Pass | `cli/tests/cli.rs::analyze_score_info_and_exit_codes`, `offload_and_selftest`; `clippro-cli selftest` 10/10 from the installed copy |
| OTH-4 | Human | Changelog viewer (What's new) is in Help. Auto-update needs **HC-8** (releases repo and update-signing key) |
| OTH-5 | Pass | `app::diag::tests::*`, `ui: views/Diagnostics.test.ts` |

## Hardening (P14.6)
| Check | Result |
|---|---|
| Fuzzing | Stable-Rust mutation fuzzer (`core/tests/fuzz.rs`) on the timeline importers (EDL, FCP7, FCPXML, OTIO, AAF), measurement blobs and audio sets. The first 3 s run found an unbounded allocation from a corrupted measurement blob (fixed). 30-minute run per target: see below. `cargo fuzz` needs nightly and libFuzzer, which this toolchain doesn't use |
| Licences | `scripts/licenses.py`: 316 Rust crates and 55 npm packages, all on the allow list. Notices ship in the installer (`licenses/`) |
| Accessibility | WCAG AA contrast for every text token on every surface in both themes (`ui: lib/contrast.test.ts`), after raising tertiary text and splitting accent fill and text colours. Keyboard-only walk-through: **Human** (part of HC-9) |
| AI throughput | `core/tests/ai.rs::throughput_with_ai_running`: 3–6% analysis slowdown with GPU AI, about 0% with CPU AI (limit 15%) |
| Memory soak | `score/tests/soak.rs`: 100 h project re-scored continuously for 15 minutes (the owner chose 15 minutes over PLAN's hour); see below |
| Performance (SPEC §22) | Met except 4K ProRes (about 1.9× in batches vs 4×): decode-bound, ADR-003 and ADR-006 |
| Clippy | No deny-level findings (`scripts/verify.ps1`) |

## Packaging (P14.1–P14.3)
| Item | Status |
|---|---|
| NSIS installer | `ClipPro_0.1.0_x64-setup.exe`, per-user, with the WebView2 bootstrapper embedded. It ships FFmpeg (LGPL, DLLs and `ffmpeg.exe`), `DirectML.dll`, `clippro-cli.exe`, the notices and the `.clippro` association. A silent install into a test folder passed `--pdf-check` and `clippro-cli selftest` with no dev tools on PATH. Uninstall removed the files and registry entries, except the `.clippro` extension key, which the stock Tauri uninstaller leaves with a backup value |
| Portable ZIP | `scripts/package-portable.ps1` |
| Signing | **HC-7** (certificate, or accept unsigned builds with SmartScreen warnings) |
| Updater | **HC-8** |
| Windows Sandbox test | Not run (installed into a test folder on the dev machine instead) |

## Owner checks still open
HC-1 (CI remote), HC-2 (calibration on real footage), HC-3 (NLE imports), HC-4 (AAF in Avid), HC-6 (faces on real footage), HC-7 (signing), HC-8 (updater), HC-9 (look approval and keyboard walk-through).

# What's new

## 1.0.2

Clearer progress, and an export sheet that fits your screen.

### Fixed

- **No more 0% at the end of a clip.** After measuring every frame, a clip finishes with its audio and a final pass, which can take a while on long footage. Progress used to drop to 0% during that step; it now stays at 100% and says **Finishing**.
- **Transcription shows its progress.** The status bar shows how far transcription has got, and the Transcript tab shows a progress bar for the clip being transcribed. It also tells you when a clip is queued behind others, still being analysed, or has no speech.
- **The export sheet fits the window.** On smaller screens the bottom of the export sheet, including the **Exportâ€¦** button, was cut off and couldn't be scrolled to. Sheets now scroll, and their buttons stay pinned at the bottom. To render clips, choose **Rendered clips** and press **Exportâ€¦**.

## 1.0.1

Two fixes.

### Fixed

- **Analysis no longer stops when you close Settings.** Closing the Settings window, or finishing a PDF contact sheet export, closed the open project behind the scenes: analysis froze at its current percentage and scoring failed with "No project is open". If this happened to you, reopen ClipPro and the analysis carries on from where it stopped.
- **Level shots are no longer flagged for horizon tilt.** A whiteboard, table or ceiling edge seen at an angle could read as a tilted horizon in a perfectly level shot, and cost interview and talking-head clips their selects. ClipPro now checks vertical lines and level lines too, so only a camera that is actually rolled is flagged.

Because the measurements changed, each project analyses its clips again once, the first time you open it in 1.0.1. Your tuning, presets, overrides and AI results are kept.

## 1.0.0

The first public release.

- **Updates**: ClipPro checks for new versions at start-up and shows their release notes; nothing installs until you choose **Install and restart**. Settings Ã¢â€ â€™ General Ã¢â€ â€™ Updates turns the check off.
- **Data folder**: keep AI models and the measurement cache on any drive (Settings Ã¢â€ â€™ General).
- **Local AI**, on your PC and only if you turn it on: People from faces, shot types, an aesthetic score, transcripts with filler and flub detection, speaker turns, search by spoken words or by describing a shot, sentence-aware selects, best-take filtering, and subtitles (SRT, WebVTT).
- **AAF** for Avid Media Composer: export the selects as a sequence with coloured markers, and review AAF edits (beta until confirmed in Media Composer).
- **Review a timeline**: open an edit from Premiere Pro, Resolve, Final Cut Pro or an EDL, see every shot's score, and swap weak shots for better takes before sending it back.
- **Multicam sync** by audio or timecode, exported as a Final Cut multicam clip or as stacked tracks.
- **Stringouts** grouped by clip, camera, date, reel, rating or colour label, with titled slugs between groups.
- **Verified offload** to several drives at once, with ASC MHL history.
- **Proxies** and **rendered selects**, with timecode kept.
- **Sound**: loudness, clipping, wind, hum and speech measures, audio flags, meters and waveforms.
- **Command line** additions: `offload`, `proxy`, `render`, `bench` and `selftest`.

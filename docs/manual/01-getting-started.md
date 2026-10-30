# Getting started

ClipPro watches your footage for you. It measures focus, exposure, motion and sound across every clip, finds the usable stretches (selects), and sends them to your editor as a timeline or as files.

## Your first project

1. Choose **New project** on the start screen and save the `.clippro` file anywhere. The project is a single file; nothing is copied into it.
2. Click **+** in the title bar, or drag files and folders onto the window. Folders are searched for media in every subfolder.
3. Analysis starts straight away. The title bar shows overall progress, and the clip list shows each clip as waiting, running or done.
4. As clips finish, selects appear on the timeline and in the score chart. You don't need to wait for everything to finish before you start tuning.
5. When the selects look right, press `Ctrl+E` to export them to Premiere Pro, Resolve, Final Cut Pro, Avid or a report.

## The window

- **Library** (left): all clips, the selects, bins, sync groups and watch folders, with the clip list below.
- **Viewer** (centre): plays the selected clip, with overlays for focus, exposure and motion.
- **Score chart**: every select as a dot, by length and score. Drag the dashed lines to change the score threshold.
- **Timeline** (bottom): every clip in a row, with its score bar, selects, flags and audio waveform.
- **Inspector** (right): tuning (presets, sliders, weighting, flags) and the frame inspector. Hide it with the panel button in the title bar.
- **Status bar**: analysis progress and time left, background jobs, and the selects total.

## How selects are chosen

Each clip is sampled several times a second. Every sample gets a score from 0 to 100, built from the measures you weight most. A select is a run of samples whose average score falls between the threshold handles and whose length fits the length limits. Flags such as Shaky or Solid colour can push a stretch out, or force it in.

Nothing is decided for good: change a slider and the selects update at once, from the measurements already taken. Analysis runs once per clip.

## Next steps

- [Tuning the selects](03-tuning.md)
- [Presets](04-presets.md)
- [Exporting](07-export.md)

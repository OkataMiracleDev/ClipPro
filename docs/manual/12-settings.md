# Settings and shortcuts

Open Settings with `Ctrl+,` or the gear button.

- **General**: theme (system, light or dark), window controls (macOS or Windows style), reopening the last project, the **Data folder**, and exporting or importing all your settings.

## Data folder

AI models and the measurement cache live in one folder, `%LOCALAPPDATA%\ClipPro` unless you choose another (for example on a bigger drive). In **Settings → General → Data folder**, choose **Change…**. With **Move existing models and cache** on, what's already there moves with it, so nothing is downloaded or analysed again. If a project is open, the cache moves the next time ClipPro starts. **Use default** goes back to the standard folder. Projects are saved wherever you choose, and importing a settings file never changes this PC's data folder.
- **Performance**: hardware decoding and the number of workers.
- **Analysis**: the default sample rate, pair sampling and which groups to measure.
- **Keyboard**: every shortcut, rebindable. Click a shortcut and press the new keys; clashes are shown before you confirm.
- **Cache**: where measurements are cached, how much space they use, and a button to clear them.
- **Compatibility**: colour-blind-safe score colours (blue to orange instead of red to green).
- **AI Models**: the optional on-device AI modules, their downloads, the transcript model and language, and Deterministic AI. Nothing downloads without your consent. See Local AI.

## Default shortcuts

| Group | Action | Keys |
|---|---|---|
| Playback | Play / pause | `Space` |
| Playback | Shuttle backwards, stop, forwards | `J` `K` `L` |
| Playback | Previous / next frame | `←` `→` |
| Playback | Back / forward one second | `Shift+←` `Shift+→` |
| Playback | Go to start / end | `Home` `End` |
| Playback | Loop the select | `Ctrl+L` |
| Editing | Mark in / out | `I` `O` |
| Editing | Force in / out | `F` |
| Editing | Trim select start / end to playhead | `[` `]` |
| Editing | Add a select between the marks | `M` |
| Viewer | Zebras, false colour, focus peaking | `Z` `C` `P` |
| Viewer | Focus map, motion vectors, waveform | `T` `V` `W` |
| Viewer | Before / after split | `B` |
| Viewer | Frame inspector | `Ctrl+I` |
| Viewer | Close A/B compare | `Esc` |
| General | Undo / redo | `Ctrl+Z` `Ctrl+Shift+Z` |
| General | Export | `Ctrl+E` |
| General | Save project as | `Ctrl+Shift+S` |
| General | Settings | `Ctrl+,` |
| General | Help | `F1` |

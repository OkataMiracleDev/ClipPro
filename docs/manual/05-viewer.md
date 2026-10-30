# The viewer

## Playback

| Key | Action |
|---|---|
| `Space` | Play or pause |
| `J` `K` `L` | Shuttle backwards, stop, forwards (press again to go faster, up to 8×) |
| `←` `→` | Previous or next frame |
| `Shift+←` `Shift+→` | Back or forward one second |
| `Home` `End` | Start or end of the clip |
| `Ctrl+L` | Loop the current select |

Clips play natively when the codec allows it; otherwise ClipPro decodes the frames itself, frame-accurately, with sound.

## Overlays

| Key | Overlay |
|---|---|
| `Z` | Zebras (two thresholds) |
| `C` | False colour |
| `P` | Focus peaking |
| `T` | Focus map: tile sharpness as a heat map |
| `V` | Motion vectors, with the camera-move gizmo (pan or tilt arrow, zoom ring, rotation arc) |
| `W` | Luma waveform monitor |

## Log footage

Log footage looks flat. Choose the camera's log profile from the colour menu in the viewer and ClipPro shows it as normal video. The profiles are S-Log3, LogC3, LogC4, V-Log, C-Log2, C-Log3, N-Log, F-Log2, D-Log, Apple Log, RED Log3G10 and BMD Film Gen5, and you can load your own `.cube` LUT. The profile is set per clip and guessed when you import. The Luma measure uses it too.

## Frame inspector

`Ctrl+I` shows everything about the current sample: the overall score, each group and sub-metric, the raw measurements, the flags, and whether the sample is in a select and why not ("Force-excluded by Shaky").

## Comparing

- `B` splits the frame: original on one side, the log preview on the other. Drag the handle.
- **A/B compare**: Alt-click another select on the timeline to play the two side by side. `Esc` closes it.

## Audio

When ClipPro plays the sound itself, the meters show each channel. Click a channel to mute it; Alt-click to solo it. If a clip has several audio streams, right-click it in the clip list and choose **Audio stream ›**.

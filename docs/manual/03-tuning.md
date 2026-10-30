# Tuning the selects

Every change re-scores instantly from the stored measurements. Undo with `Ctrl+Z` and redo with `Ctrl+Shift+Z`.

## Selects

- **Score threshold**: two handles. A stretch becomes a select when its average score sits between them. Raise the low handle to be stricter.
- **Segment length**: the shortest and longest select. Type values such as `4`, `4.5`, `1:30`, `90s` or `none`.
- **Max per clip**: keeps only the best selects of each clip (`none` for no limit).

## Weighting

The radar has a corner for each group: Focus, Exposure, Motion and Audio. Drag a corner to make that group count more; drag it to the centre to leave the group out. `Ctrl`+drag moves one corner without rebalancing the others.

Open a group below the radar to tune its sub-metrics. Each slider runs from "prefer less" through Neutral to "prefer more"; how far you move it sets how much it matters. **Luma** and **Contrast** work differently: you set a target level and a tolerance. They're off until you switch them on.

## Camera movement

The camera wheel keeps only stretches where the camera moves the way you want:

- Drag on the wheel to choose a pan or tilt direction and how wide a spread to accept.
- The outer arcs accept zooms and rotations.
- **Static only** keeps locked-off shots.
- **Mirror** accepts the opposite direction too (left or right, for instance).

## Flags

Flags mark problems (and some features) sample by sample:

Clipping, Duplicate, Solid colour, Hard cut, Motion blur, Shaky, Low light, Focus hunting, Colour bars, Horizon tilt, Audio clipping, Silence, Wind, Hum, Dead channel, Channel imbalance.

Each flag has five positions: **Force exclude**, **Disfavour** (score ×0.6), **Neutral**, **Favour** (×1.2) and **Force include**. The number beside a flag counts its runs across the project; click it to jump through them.

## Filters

The **Metrics**, **Flags** and **Camera movement** switches turn each filter off for a moment, so you can see what it's doing.

## Advanced

The Advanced drawer holds the gap tolerance (how short a dip can be bridged), centre weighting, stability emphasis and handles (extra frames exported around each select).

## Your own decisions

- Select a stretch on the timeline and press `F` to force it in or out.
- Mark in and out with `I` and `O`, then press `M` to add a select by hand.
- `[` and `]` trim the current select to the playhead.
- Give a select a star rating, a colour label and a note in the select editor. Exports can carry them in markers.

## Snapshots and A/B

Save the current settings as a named snapshot to come back to later. **A/B** keeps your current tuning as A while you try changes as B; click it to switch between the two and see the selects change.

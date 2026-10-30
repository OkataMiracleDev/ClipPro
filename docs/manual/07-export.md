# Exporting

Press `Ctrl+E` (or the share button) to export the current selects.

## Formats

| Format | For |
|---|---|
| Premiere Pro / Resolve | FCP7 XML |
| Final Cut Pro | FCPXML 1.10 (Resolve reads it too) |
| OpenTimelineIO | Resolve 18.5 and later, and OTIO tools |
| EDL | CMX3600, with locators for the markers |
| Avid Media Composer | AAF: the sequence, with markers in their colours, linked to the original files (beta until confirmed in Media Composer) |
| Avid ALE | Subclips for Media Composer |
| Avid markers | A marker list for Media Composer's Import Markers |
| CSV, JSON | Reports for spreadsheets and scripts |
| HTML report | One self-contained file with thumbnails and sortable tables |
| PDF contact sheet | The best frame of every select, for clients |
| Rendered clips | A media file per select |

## Layout

- **Selects only**: the selects one after another on one track, with an optional gap between them.
- **With rejects on V2**: whole clips in order, the selects on V1 and the rest on V2 beneath them, so nothing is lost.
- **Stringout**: the selects grouped by clip, camera, date recorded, reel, rating or colour label. Groups play A to Z (oldest first for dates, best first for ratings and labels) or with the group holding the best select first. Between groups you can leave nothing, a gap, or a black slug titled with the group's name.

**Include handles** adds the extra frames set in the Advanced drawer around each select.

## Markers

Each select can carry a marker. Its name and comment are templates; these tokens are filled in:

`{clip}` `{index}` `{score}` `{flags}` `{duration}` `{tc_in}` `{tc_out}` `{rating}` `{label}` `{note}`

With local AI on (see Local AI), four more: `{transcript}` (what is said during the select), `{people}` (who is on screen), `{shot_type}` (also `{shot}`) and `{take}` ("Take 2" when the select is one of several takes). The comment defaults to `{flags}`.

The marker colour follows the score (green, yellow, red) unless you gave the select a colour label.

## Rendered clips

Writes one QuickTime file per select into a folder you choose, with the source timecode at its start.

- **Copy without re-encoding** is fastest and keeps the original quality. It works when the select starts on a keyframe. Allow **starting at the keyframe before the in point** to copy more often (the file then starts a little early). A copy can also run a frame or two past the out point.
- Otherwise each select is encoded as ProRes 422, ProRes 422 HQ, DNxHR HQ or H.264 (on your graphics card's encoder where it has one).
- **File names** use the tokens `{clip}` `{index}` `{tc_in}` `{tc_out}` `{score}` `{rating}` `{label}`.

Rendering runs in the background; the status bar shows progress and a Stop button.

## Where media points

Exports always point at the original files, even when ClipPro analysed a proxy.

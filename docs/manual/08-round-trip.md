# Reviewing an edit, multicam and stringouts

## Review a timeline

Score an edit someone has already made, and swap weak shots for better takes.

1. Click the clipboard button in the title bar (**Review a timeline…**) and choose an AAF, FCP7 XML, FCPXML, OTIO or EDL file.
2. ClipPro finds each shot's media in the project: by path, then file name, then clip name, then reel and timecode. Shots it can't find are listed as offline.
3. Each shot shows its score bar and average over the part the edit uses, plus any flags. Shots below the **Weak below** score, or containing a force-excluded moment, are marked weak.
4. Open a shot to see better alternatives: other stretches of the same clip, or other takes of the same set-up (judged by how alike the pictures are). **Show in viewer** plays one; choose it to swap it in.
5. **Export back…** writes the edit in any timeline format. Tracks and positions stay as they were; swapped shots keep the original length, and each shot gets a `ClipPro` marker with its score. Exporting the same edit again replaces those markers rather than adding more.

EDLs have no frame rate; ClipPro uses your project's most common one.

## Multicam sync

Line up cameras that recorded the same moment.

1. Right-click a clip and choose **Sync with other clips…**, or click **+** beside Sync groups in the sidebar.
2. Tick the clips and choose **Audio** (matches the sound each camera recorded) or **Timecode** (lines up start timecodes).
3. ClipPro shows each angle's offset from the reference (the longest clip, or the earliest timecode). PSR is the confidence of an audio match; 6 or more is accepted, and well above 10 is typical. If a clip has both audio and timecode and they disagree by more than a frame, you'll see a warning.
4. **Save group**, then **Export…** as a Final Cut multicam clip (FCPXML) or as stacked tracks in XML or OTIO for Premiere and Resolve.

Clips that don't overlap the reference can still be synced through another clip that overlaps both.

## Stringouts

Choose **Stringout** as the layout in the export sheet. See [Exporting](07-export.md).

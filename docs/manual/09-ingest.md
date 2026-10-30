# Ingest

## Offloading cards

The drive button in the title bar (**Offload a card…**) copies a card or folder to one or more drives at once and proves every copy.

1. Choose the source folder and add destinations. Each gets a folder named after the source.
2. Choose a checksum: xxHash64 (fast, recommended), xxHash3, xxHash128, MD5 or SHA-1.
3. Start. The card is read once and every destination is written at the same time.
4. Each copy is read back and checked against the source before it gets its real name. A copy that doesn't match is reported and never left in place.

Each destination gets an `ascmhl` folder: an ASC Media Hash List history any MHL tool can verify later. If the offload stops (you cancel, or a drive fills), run it again: files already verified are skipped.

Turn on **Add the first destination's copy to this project** to start analysing as soon as the copy is verified.

## Watch folders

Add a folder under Watch folders in the sidebar and new media in it is imported and analysed automatically. ClipPro waits until a file has stopped growing, so it's safe to point at a folder that's still being copied to. Each folder can include subfolders and be limited to certain extensions.

## Making proxies

Right-click a clip and choose **Make proxy ›**:

- ProRes 422 Proxy or LT, DNxHR LB, or H.264 (the list shows only what works on this computer).
- Proxies are at most 1080 lines high, keep the timecode, reel name and sound, and go in a `Proxy` folder beside the clip, named `<clip>_Proxy.mov`.
- The proxy is linked straight away, and the clip is analysed from it.

## Missing media

If files move, clips show as missing. Right-click one and choose **Relink this clip…** to point at the new file, or **Find missing files in a folder…** to search a folder for every missing clip at once.

## Duplicates

When you import a file that's already in the project (the same content, even under another name), it's skipped and reported.

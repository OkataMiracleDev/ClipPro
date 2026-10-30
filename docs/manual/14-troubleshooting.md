# Troubleshooting

**A clip failed to analyse.** Right-click it and choose **Retry**. If the graphics driver is the problem, **Retry with CPU decoding** usually works. The clip's tooltip shows the error.

**Analysis is slow.** Use a lower sample rate (Draft or Fast) for triage, make sure hardware decoding is on, and analyse proxies of very large camera originals. `clippro-cli bench` measures your machine. ProRes and other intra-frame codecs are limited by decoding speed.

**Clips show as missing.** The files moved. Right-click and choose **Relink this clip…**, or **Find missing files in a folder…** to find them all at once.

**Nothing is selected.** Lower the low threshold handle, shorten the minimum length, or check the flags: a force-excluded flag removes every stretch it touches. The **Flags** and **Metrics** switches show which filter is responsible, and the frame inspector (`Ctrl+I`) explains any single sample.

**My NLE won't open the export.** Check the format matches the application (FCP7 XML for Premiere Pro, FCPXML for Final Cut Pro). Media paths point at the original files; if they've moved since, relink in the NLE.

**An offload reports a failed copy.** The destination drive returned different data from the source. Run the offload again to re-copy only those files; if it keeps failing, the drive or its cable may be faulty.

**Rendering or proxies say no encoder works.** ProRes and DNxHR always work. H.264 uses your graphics card's encoder (NVIDIA, AMD or Intel) and falls back to a software one.

**Checking the installation.** `clippro-cli selftest` makes a few test clips, analyses and exports them, and reports each step.

"""Authors a small reference AAF with pyaaf2: the shape ClipPro writes (P7b).

Usage: PYTHONPATH=third_party/pyaaf2 python scripts/aaf/make_reference.py out.aaf
"""
import sys

import aaf2

out = sys.argv[1]
with aaf2.open(out, "w") as f:
    rate = "25"
    comp = f.create.CompositionMob("ClipPro Selects")
    f.content.mobs.append(comp)
    ctc = comp.create_timeline_slot(rate)
    ctc.segment = f.create.Timecode(25, drop=False, length=251)
    ctc.segment["Start"].value = 90000
    seq = f.create.Sequence(media_kind="picture")
    slot = comp.create_timeline_slot(rate)
    slot.segment = seq
    for i, name in enumerate(["A001C003", "A002C004"]):
        # Tape/file source with a network locator, then a master mob over it.
        src = f.create.SourceMob(name)
        desc = f.create.CDCIDescriptor()
        desc["ComponentWidth"].value = 8
        desc["HorizontalSubsampling"].value = 2
        desc["ImageAspectRatio"].value = "16/9"
        desc["StoredWidth"].value = 1920
        desc["StoredHeight"].value = 1080
        desc["FrameLayout"].value = "FullFrame"
        desc["VideoLineMap"].value = [42, 0]
        desc["SampleRate"].value = rate
        desc["Length"].value = 250
        loc = f.create.NetworkLocator()
        loc["URLString"].value = "file:///D:/Footage/%s.mov" % name
        desc["Locator"].append(loc)
        src.descriptor = desc
        src.create_timeline_slot(rate).segment = f.create.SourceClip(media_kind="picture", length=250)
        tc = src.create_timecode_slot(rate, 25, drop_frame=False, length=250)
        tc.segment["Start"].value = 900000 + i * 1000
        f.content.mobs.append(src)
        master = f.create.MasterMob(name)
        mslot = master.create_timeline_slot(rate)
        mslot.segment = src.create_source_clip(slot_id=1, length=250)
        f.content.mobs.append(master)
        clip = master.create_source_clip(slot_id=1, start=6, length=113)
        seq.components.append(clip)
        if i == 0:
            seq.components.append(f.create.Filler("picture", 25))
    # A marker on the composition (event slot with a DescriptiveMarker).
    ev_slot = f.create.EventMobSlot()
    ev_slot["EditRate"].value = rate
    ev_slot["SlotID"].value = 1000
    mseq = f.create.Sequence(media_kind="DescriptiveMetadata")
    marker = f.create.DescriptiveMarker()
    marker["Position"].value = 19
    marker["Length"].value = 75
    marker["Comment"].value = "Select 1 - 82"
    marker["CommentMarkerColor"].value = {"red": 0, "green": 65535, "blue": 0}
    marker["DescribedSlots"].value = set([1])
    mseq.components.append(marker)
    ev_slot.segment = mseq
    comp.slots.append(ev_slot)
print("wrote", out)

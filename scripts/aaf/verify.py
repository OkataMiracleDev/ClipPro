"""Reads an AAF with pyaaf2 (an independent implementation) and prints a JSON summary of the
top-level composition: picture tracks (clips and fillers, with media URL and source timecode),
the record timecode and the markers. Every property of every object is decoded on the way, so
a malformed file fails here. Used by ClipPro's AAF tests (PLAN P7b).

Usage: PYTHONPATH=third_party/pyaaf2 python scripts/aaf/verify.py file.aaf
"""
import json
import sys

import aaf2


def decode_all(obj, seen):
    if id(obj) in seen:
        return
    seen.add(id(obj))
    for p in obj.properties():
        v = p.value
        if isinstance(v, aaf2.core.AAFObject):
            decode_all(v, seen)
        elif isinstance(v, (list, tuple)):
            for x in v:
                if isinstance(x, aaf2.core.AAFObject):
                    decode_all(x, seen)


def media_of(clip):
    """Follows SourceClip -> MasterMob -> SourceMob to (name, url, tc_start)."""
    mob = clip.mob
    name = mob.name if mob is not None else None
    url, tc = None, None
    for _ in range(4):
        if mob is None:
            break
        if "EssenceDescription" in mob.keys():
            d = mob.descriptor
            locs = d["Locator"].value if "Locator" in d.keys() else []
            url = locs[0]["URLString"].value if locs else None
            for s in mob.slots:
                if s.segment.name == "Timecode":
                    tc = s.segment["Start"].value
            break
        nxt = None
        for s in mob.slots:
            if s.segment.name == "SourceClip":
                nxt = s.segment.mob
                break
        mob = nxt
    return name, url, tc


with aaf2.open(sys.argv[1], "r") as f:
    decode_all(f.root, set())
    comps = [m for m in f.content.mobs if m.name is not None and type(m).__name__ == "CompositionMob"]
    comp = comps[0]
    out = {"name": comp.name, "mobs": len(list(f.content.mobs)), "tracks": [], "timecode": None, "markers": [],
           "usage": str(comp["UsageCode"].value) if "UsageCode" in comp.keys() else None,
           "product": f.header["IdentificationList"].value[0]["ProductName"].value}
    for slot in comp.slots:
        seg = slot.segment
        if seg.name == "Timecode":
            out["timecode"] = {"start": seg["Start"].value, "fps": seg["FPS"].value, "drop": bool(seg["Drop"].value)}
            continue
        if type(slot).__name__ == "EventMobSlot":
            for m in seg.components:
                c = m["CommentMarkerColor"].value if "CommentMarkerColor" in m.keys() else None
                out["markers"].append({"position": m["Position"].value, "length": m.length, "comment": m["Comment"].value,
                                       "color": [c["red"], c["green"], c["blue"]] if c else None,
                                       "described": sorted(m["DescribedSlots"].value) if "DescribedSlots" in m.keys() else []})
            continue
        items = []
        rate = slot["EditRate"].value
        for c in seg.components:
            if c.name == "SourceClip":
                name, url, tc = media_of(c)
                items.append({"kind": "clip", "length": c.length, "start": c.start, "mob": name, "url": url, "tc": tc})
            else:
                items.append({"kind": c.name.lower(), "length": c.length})
        out["tracks"].append({"slot": slot.slot_id, "rate": [rate.numerator, rate.denominator], "length": seg.length, "items": items})
    print(json.dumps(out, indent=1))

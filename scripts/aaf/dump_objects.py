"""Dumps every object under the AAF header (not the MetaDictionary): path, class, and each
property's pid, name, stored format and raw bytes. Usage: dump_objects.py file.aaf"""
import sys

import aaf2
from aaf2 import properties as P


def walk(obj, path, seen):
    if id(obj) in seen:
        return
    seen.add(id(obj))
    print("%s  [%s] clsid=%s dir=%s" % (path, obj.classdef.class_name if obj.classdef else "?", obj.class_id, obj.dir.path() if obj.dir else None))
    for p in obj.properties():
        kind = type(p).__name__
        print("    %04x %-28s fmt=%02x %-24s %s" % (p.pid, p.name, p.format, kind, (p.data or b"").hex()))
        if isinstance(p, P.StrongRefProperty):
            walk(p.value, path + "/" + p.name, seen)
        elif isinstance(p, P.StrongRefVectorProperty):
            for i, v in enumerate(p.value):
                walk(v, "%s/%s[%d]" % (path, p.name, i), seen)
        elif isinstance(p, P.StrongRefSetProperty):
            if p.name in ("DataDefinitions", "ContainerDefinitions", "CodecDefinitions", "OperationDefinitions", "ParameterDefinitions", "InterpolationDefinitions", "PluginDefinitions", "TaggedValueDefinitions", "KLVDataDefinitions"):
                print("      (%d definitions)" % len(p.value))
                continue
            for i, v in enumerate(p.value):
                walk(v, "%s/%s{%d}" % (path, p.name, i), seen)


with aaf2.open(sys.argv[1], "r") as f:
    print("referenced properties:", f.weakref_table)
    walk(f.root, "", set())

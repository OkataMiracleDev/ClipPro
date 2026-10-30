"""Lists the storages and streams of a compound file (AAF), with sizes; `-v` hex-dumps
"properties" streams up to a depth. Usage: dump_cfb.py file.aaf [maxdepth] [-v]"""
import sys

from aaf2.cfb import CompoundFileBinary

path = sys.argv[1]
maxdepth = int(sys.argv[2]) if len(sys.argv) > 2 and sys.argv[2].isdigit() else 99
verbose = "-v" in sys.argv
with open(path, "rb") as fh:
    cfb = CompoundFileBinary(fh, "rb")

    def walk(entry, depth):
        if depth > maxdepth:
            return
        for e in cfb.listdir(entry):
            kind = "D" if e.isdir() else "S"
            clsid = str(e.class_id) if e.isdir() and e.class_id else ""
            size = e.byte_size if not e.isdir() else ""
            print("  " * depth + "%s %s %s %s" % (kind, e.name, size, clsid))
            if verbose and not e.isdir() and e.name == "properties":
                s = cfb.open(e.path(), "r")
                data = s.read()
                print("  " * depth + "    " + data.hex())
            if e.isdir():
                walk(e, depth + 1)

    walk(cfb.find("/"), 0)

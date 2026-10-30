"""Builds the empty AAF template ClipPro's AAF writer fills in (PLAN P7b): the full baseline
MetaDictionary, a Header with the default Dictionary (data definitions) and an empty
ContentStorage. Regenerate only when changing the template; it's committed.

Usage: PYTHONPATH=third_party/pyaaf2 python scripts/aaf/make_template.py \
           crates/clippro-interchange/assets/aaf-template.aaf
"""
import sys

import aaf2
from aaf2.auid import AUID

out = sys.argv[1]
with aaf2.open(out, "w") as f:
    ident = f.header["IdentificationList"].value[0]
    ident["ProductName"].value = "ClipPro"
    ident["CompanyName"].value = "ClipPro"
    ident["ProductVersionString"].value = "1.0"
    ident["ProductID"].value = AUID("5f3b9c2e-6a41-4d8e-9b57-c1a0e2f4d6b8")
    ident["Platform"].value = "Windows"
    # Entry 2 of the weak-reference table is Header → Dictionary → DataDefinitions, which every
    # component's DataDefinition points through (as in files with content).
    f.weakref_index([0x02, 0x3B04, 0x2605])
    print("weakref table:", f.weakref_table)
print("wrote", out)

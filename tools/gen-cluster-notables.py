"""Writes crates/pob-optimizer/src/cluster_notables.json from upstream's Data/ModJewelCluster.lua: which notables can appear on a cluster jewel of which small-passive type.
Run again after an upstream update:  python tools/gen-cluster-notables.py <PathOfBuilding checkout>"""
import json
import os
import re
import sys

root = sys.argv[1]
text = open(os.path.join(root, "src", "Data", "ModJewelCluster.lua"), encoding="utf-8").read()
line = re.compile(r'type = "(Prefix|Suffix)".*?"1 Added Passive Skill is ([^"]+)".*?weightKey = \{([^}]*)\}, weightVal = \{([^}]*)\}')
out = {}
for m in line.finditer(text):
    kind, name, keys, vals = m.groups()
    keys = re.findall(r'"([^"]+)"', keys)
    vals = [int(v) for v in re.findall(r"-?\d+", vals)]
    for key, val in zip(keys, vals):
        if val > 0 and key != "default":
            entry = out.setdefault(key, {"prefix": [], "suffix": []})
            bucket = entry[kind.lower()]
            if name not in bucket:
                bucket.append(name)
for entry in out.values():
    entry["prefix"].sort()
    entry["suffix"].sort()
target = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "pob-optimizer", "src", "cluster_notables.json")
with open(target, "w", encoding="utf-8", newline="\n") as f:
    json.dump(out, f, indent=0, sort_keys=True)
    f.write("\n")
print("tags", len(out), "notables", len({n for e in out.values() for b in e.values() for n in b}))

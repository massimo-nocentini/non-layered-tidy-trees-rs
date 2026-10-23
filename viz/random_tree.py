"""The 60-box random tree of the first picture, from the prototype's own generator.

Writes `viz/out/random.tree` and `viz/out/random.json`.
"""
import json
import os
import random
import sys

from pg_common import HERE, REPO, write_tree

sys.path.insert(0, os.path.join(REPO, "prototype"))
import proto  # noqa: E402

t = proto.randtree(60, random.Random(6), maxk=4, sizes=(2, 6), deep=0.0)
parent = [-1] * len(t.w)
for v in range(len(t.w)):
    for c in t.ch[v]:
        parent[c] = v
assert all(parent[v] < v for v in range(1, len(t.w))), "the generator numbers parents first"

os.makedirs(os.path.join(HERE, "out"), exist_ok=True)
write_tree(os.path.join(HERE, "out", "random.tree"),
           [dict(w=float(t.w[v]), h=float(t.h[v]), d=float(t.d[v]), margin=0.0, parent=parent[v]) for v in range(len(t.w))])
json.dump(dict(key="random", title="Random tree",
               blurb="A random tree of 60 boxes with integer sizes from 2 to 6, the one of the first picture.",
               time=None, facts=dict(nodes=len(t.w), cross=0), meta=None, cross=[]),
          open(os.path.join(HERE, "out", "random.json"), "w"))
print("random", len(t.w), "boxes")

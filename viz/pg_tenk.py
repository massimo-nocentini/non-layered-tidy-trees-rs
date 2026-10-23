"""The whole Payment Graph of the first 10,000 transactions, made one tree.

Read backwards from the outputs still unspent when the window ends: each is the root of
its own provenance, the inputs of the transaction that created an output being its
children, and every output of the window is an ancestor of one of them. A node reached
twice keeps its first parent; the other arcs are kept aside, to be drawn dashed.

An invisible grouping tree joins the roots into one tree: four levels of at most ten, over
the roots in chronological order. Its nodes have no size and their families are fixed by
level -- x just above the roots, then z, x, z -- so that the roots pack into a square. Left
to `Alternate`, the few groups that hold a deeper provenance would get the other parity
from their subtree height, turn the wrong way, and spread the square six times wider.

Box height is the output's lifetime in blocks within the window, the footprint side the
square root of its value in BTC.

Writes `tests/data/pg_tenk.tree` and `viz/out/tenk.json`.
"""
import collections
import json
import math
import os

from pg_common import REPO, HERE, coinbase, components, created, of, succ, tx, value, write_tree

N = 10000
FAN = 10
LEVELS = 3   # of groups below the root

keep = {v for v in of if of[v][0] < N}
s2 = collections.defaultdict(list)
p2 = collections.defaultdict(list)
for u in keep:
    for v in succ[u]:
        if v in keep:
            s2[u].append(v)
            p2[v].append(u)
END = max(tx[of[v][0]]["height"] for v in keep)


def spent(v):
    return created(s2[v][0]) if s2[v] else None


roots = sorted((v for v in keep if not s2[v]), key=lambda v: (created(v), v))


def groups(items, depth):
    if depth == 0:
        return items
    size = math.ceil(len(items) / FAN)
    return [groups(items[i:i + size], depth - 1) for i in range(0, len(items), size)]


def leaves(g):
    return [y for x in g for y in (leaves(x) if isinstance(x, list) else [x])]


nodes = [dict(group=True, level=0, lo=roots[0], hi=roots[-1], parent=-1)]


def attach(sub, parent, level):
    for g in sub:
        if isinstance(g, list):
            flat = leaves(g)
            nodes.append(dict(group=True, level=level, lo=flat[0], hi=flat[-1], parent=parent))
            attach(g, len(nodes) - 1, level + 1)
        else:
            nodes.append(dict(group=False, v=g, parent=parent))


attach(groups(roots, LEVELS), 0, 1)

index = {n["v"]: i for i, n in enumerate(nodes) if not n["group"]}
cross = []
queue = [i for i, n in enumerate(nodes) if not n["group"]]
for i in queue:
    for c in sorted(p2[nodes[i]["v"]]):
        if c in index:
            cross.append([i, index[c]])
        else:
            nodes.append(dict(group=False, v=c, parent=i))
            index[c] = len(nodes) - 1
            queue.append(len(nodes) - 1)
assert len(index) == len(keep)

# parents first, for the Rust side
kids = collections.defaultdict(list)
for i, n in enumerate(nodes):
    if n["parent"] >= 0:
        kids[n["parent"]].append(i)
bfs = [0]
for i in bfs:
    bfs.extend(kids[i])
pos = {old: new for new, old in enumerate(bfs)}
nodes = [nodes[i] for i in bfs]
for n in nodes:
    n["parent"] = -1 if n["parent"] < 0 else pos[n["parent"]]
cross = [[pos[u], pos[v]] for u, v in cross]

rows, meta = [], []
for n in nodes:
    if n["group"]:
        rows.append(dict(w=0.0, h=0.0, d=0.0, margin=0.0, parent=n["parent"],
                         axis=0 if (LEVELS - n["level"]) % 2 == 0 else 1))
        meta.append([-1, created(n["lo"]), created(n["hi"])])
    else:
        v = n["v"]
        t, o = of[v]
        s = spent(v)
        side = math.sqrt(value(v) / 1e8)
        rows.append(dict(w=side, h=float((s if s is not None else END) - created(v)), d=side, margin=1.0,
                         parent=n["parent"]))
        meta.append([t, o, value(v) / 1e8, created(v), -1 if s is None else s, int(coinbase(v))])

sizes = [len(c) for c in components(keep, s2, p2)]
facts = dict(txs=N, coinbase=sum(1 for t in range(N) if tx[t]["nin"] == 0), outputs=len(keep),
             arcs=sum(len(x) for x in s2.values()), unspent=len(roots), groups=sum(n["group"] for n in nodes),
             cross=len(cross), blocks=[0, END], components=len(sizes), singles=sizes.count(1), largest=max(sizes))

os.makedirs(os.path.join(HERE, "out"), exist_ok=True)
write_tree(os.path.join(REPO, "tests", "data", "pg_tenk.tree"), rows)
json.dump(dict(key="tenk", title="First 10,000 transactions",
               blurb=f"Every output of the first 10,000 transactions, blocks 0 to {END:,}, read backwards from the "
                     f"{len(roots):,} still unspent when that window ends. Each box hangs from the top plane for as long "
                     f"as its output existed, and the provenance of the few that were spent branches out below it.",
               time=dict(forward=False, origin=END), facts=facts, meta=meta, cross=cross),
          open(os.path.join(HERE, "out", "tenk.json"), "w"))
print("tenk", facts)

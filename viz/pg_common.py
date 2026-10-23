"""The Payment Graph of the first 32,485 blocks, from the reference fixtures of utxo2webgraph-rs.

One node per transaction output, an arc from every input of a transaction to each of its
outputs. The fixtures are the pipeline of Matteo Loporchio, as reimplemented there:

* `nm_12.tsv` -- the node map, `txid  output  node`;
* `el_12.tsv` -- the arcs, `node  node`;
* `c12.txt`   -- the transactions, `info:inputs:outputs` with info = timestamp, height, txid,
  ...; an input = address, value, previous txid, previous output; an output = address,
  value, ...

`PG_REFERENCE` overrides where they are; the default is a checkout of utxo2webgraph-rs next
to this repository.
"""
import collections
import os

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
REFERENCE = os.environ.get(
    "PG_REFERENCE",
    os.path.join(os.path.dirname(REPO), "utxo2webgraph-rs", "tests", "data", "reference"))

node = {}   # (txid, output) -> node
of = {}     # node -> (txid, output)
for line in open(os.path.join(REFERENCE, "nm_12.tsv")):
    t, o, a = map(int, line.split())
    node[(t, o)] = a
    of[a] = (t, o)

succ = collections.defaultdict(list)
pred = collections.defaultdict(list)
for line in open(os.path.join(REFERENCE, "el_12.tsv")):
    u, v = map(int, line.split())
    succ[u].append(v)
    pred[v].append(u)

tx = {}     # txid -> height, output values in satoshi, number of inputs
for line in open(os.path.join(REFERENCE, "c12.txt")):
    parts = line.rstrip("\n").split(":")
    info = parts[0].split(",")
    ins = [p for p in parts[1].split(";") if p]
    outs = [p for p in parts[2].split(";") if p]
    tx[int(info[2])] = dict(height=int(info[1]), vals=[int(o.split(",")[1]) for o in outs], nin=len(ins))

LAST = max(t["height"] for t in tx.values())


def reach(r, nb):
    seen = {r}
    stack = [r]
    while stack:
        u = stack.pop()
        for v in nb[u]:
            if v not in seen:
                seen.add(v)
                stack.append(v)
    return seen


def created(v):
    return tx[of[v][0]]["height"]


def value(v):
    t, o = of[v]
    return tx[t]["vals"][o]


def coinbase(v):
    return tx[of[v][0]]["nin"] == 0


def components(nodes, nb_out, nb_in):
    """Weakly connected components of the subgraph on `nodes`, largest first."""
    seen = set()
    comps = []
    for s in sorted(nodes):
        if s in seen:
            continue
        c = {s}
        stack = [s]
        while stack:
            u = stack.pop()
            for w in nb_out[u] + nb_in[u]:
                if w in nodes and w not in c:
                    c.add(w)
                    stack.append(w)
        seen |= c
        comps.append(c)
    comps.sort(key=lambda c: (-len(c), min(c)))
    return comps


def write_tree(path, rows):
    """A tree for `examples/space_trees.rs`: one box per line, the root first and every
    parent before its children."""
    with open(path, "w") as f:
        f.write("# w h d margin parent axis -- one box per line, parents first; axis 0 x, 1 z, -1 none\n")
        for r in rows:
            f.write(f"{r['w']!r} {r['h']!r} {r['d']!r} {r['margin']!r} {r['parent']} {r.get('axis', -1)}\n")

"""Two components of the Payment Graph, each made a tree: the funnel and the first payment.

* `funnel` -- the largest component, read backwards from its one unspent output: 450
  coinbase rewards swept into output 0 of transaction 27,711. The children of an output are
  the inputs of the transaction that created it.
* `first` -- where the coinbase output of block 9 went, read forwards: the children of an
  output are the outputs of the transaction that spent it.

A node reached twice keeps the first parent a breadth-first visit gives it, and the other
arcs are kept aside, to be drawn dashed. Either way time is exact: box height is the
output's lifetime in blocks, so a child's box starts where its parent's ends. The footprint
side is the square root of the value in BTC.

Writes `tests/data/pg_{funnel,first}.tree` and `viz/out/{funnel,first}.json`.
"""
import json
import math
import os

from pg_common import LAST, REPO, HERE, coinbase, components, created, node, of, pred, succ, tx, value, write_tree


def spent(v):
    return created(succ[v][0]) if succ[v] else None


def spanning(root, nb):
    parent = {root: None}
    order = [root]
    cross = []
    for u in order:
        for v in sorted(nb[u]):
            if v in parent:
                cross.append((u, v))
            else:
                parent[v] = u
                order.append(v)
    return order, parent, cross


def case(key, title, blurb, root, nb, forward):
    order, parent, cross = spanning(root, nb)
    idx = {v: i for i, v in enumerate(order)}
    rows, meta, depth = [], [], []
    for v in order:
        t, o = of[v]
        s = spent(v)
        side = math.sqrt(value(v) / 1e8)
        p = -1 if parent[v] is None else idx[parent[v]]
        rows.append(dict(w=side, h=float((s if s is not None else LAST) - created(v)), d=side, margin=1.0, parent=p))
        meta.append([t, o, value(v) / 1e8, created(v), -1 if s is None else s, int(coinbase(v))])
        depth.append(0 if p < 0 else depth[p] + 1)
    fan = max(sum(1 for r in rows if r["parent"] == i) for i in range(len(rows)))
    facts = dict(nodes=len(order), txs=len({of[v][0] for v in order}), coinbase=sum(coinbase(v) for v in order),
                 fanin=fan, levels=max(depth) + 1, cross=len(cross), blocks=[min(created(v) for v in order), LAST])
    write_tree(os.path.join(REPO, "tests", "data", f"pg_{key}.tree"), rows)
    out = dict(key=key, title=title, blurb=blurb, time=dict(forward=forward, origin=created(root) if forward else LAST),
               facts=facts, meta=meta, cross=[[idx[u], idx[v]] for u, v in cross])
    json.dump(out, open(os.path.join(HERE, "out", f"{key}.json"), "w"))
    print(key, facts)


os.makedirs(os.path.join(HERE, "out"), exist_ok=True)
everything = set(of)
funnel = components(everything, succ, pred)[0]
sink = next(v for v in funnel if not succ[v])
transfers = sum(1 for t in {of[v][0] for v in funnel} if tx[t]["nin"] > 0)
case("funnel", "Funnel to 22,500 BTC",
     f"Where the 22,500 BTC of output 0 of transaction 27,711 came from: 450 coinbase rewards, swept together "
     f"by {transfers} transactions into one output created in block 27,528. Read backwards from that output, "
     f"the inputs of the transaction that created an output are its children.",
     sink, pred, False)
case("first", "First payment",
     "Where the 50 BTC of the block-9 coinbase went, starting with the 10 BTC to Hal Finney in block 170: "
     "the outputs of the transaction that spent an output are its children.",
     node[(9, 0)], succ, True)

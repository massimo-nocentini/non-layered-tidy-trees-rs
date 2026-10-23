# The 3D test cases page

`tidy3d.html` is a self-contained page of trees laid out in 3D by `space::layout3`: the
whole Payment Graph of the first 10,000 Bitcoin transactions, the largest component of the
first 32,485 blocks, where the block-9 coinbase went, and the random tree of the first
picture. Open it in a browser; it loads three.js from jsDelivr.

```sh
viz/build.sh        # needs python3, node, cargo, and utxo2webgraph-rs next to this repo
```

| Step | What it does |
|---|---|
| `random_tree.py` | the 60-box tree, from `prototype/proto.py`'s generator |
| `pg_cases.py`, `pg_tenk.py` | the Payment Graph trees into `tests/data/*.tree`, what each node is into `out/*.json` |
| `examples/space_trees.rs` | every tree under `Alternate`, `Compact` and `AllX`; fails unless the flat engine agrees bit for bit and no two boxes overlap |
| `assemble.py` | `page.html` filled with all of it, into `tidy3d.html` |
| `harness.js` | the page's logic run in Node against stand-ins for three.js and the DOM, through every case, rule, view, colour and the time scale |

`pg_common.py` reads the Payment Graph fixtures; `PG_REFERENCE` points it elsewhere.
`out/` is regenerated and not committed. The harness does not draw: the WebGL rendering
itself is only checked by opening the page.

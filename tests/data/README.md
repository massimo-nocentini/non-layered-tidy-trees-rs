# Payment Graph fixtures

Trees made from the Bitcoin Payment Graph, for `tests/space_payment_graph.rs` and for the
page in [`viz/`](../../viz/README.md). The Payment Graph has one node per transaction
output and an arc from every input of a transaction to each of its outputs; these come from
the reference fixtures of [utxo2webgraph-rs](../../../utxo2webgraph-rs) — `c12.txt`,
`nm_12.tsv`, `el_12.tsv`, the first 32,485 blocks — whose pipeline is the work of Matteo
Loporchio.

| File | Built by | What it is |
|---|---|---|
| `pg_tenk.tree` | `viz/pg_tenk.py` | every output of the first 10,000 transactions, read backwards from the 9,419 still unspent at block 9,909, under a zero-size grouping tree of four levels of ten |
| `pg_funnel.tree` | `viz/pg_cases.py` | the largest component, read backwards from its one output: 450 coinbases swept into output 0 of transaction 27,711 |
| `pg_first.tree` | `viz/pg_cases.py` | where the coinbase output of block 9 went, read forwards |

One box per line, `w h d margin parent axis`, the root first and every parent before its
children; `axis` is `0` for `x`, `1` for `z`, `-1` to leave it to the rule, and is set on
the grouping nodes only. A box is `√BTC` wide and deep and as high as the output's
lifetime in blocks, so that a box starts where its parent's ends and the depth axis is
block height. A node reached twice keeps one parent; the arcs left out are in
`viz/out/*.json`, not here.

`viz/build.sh` rebuilds them, and the page, from a checkout of utxo2webgraph-rs next to
this repository, or wherever `PG_REFERENCE` points.

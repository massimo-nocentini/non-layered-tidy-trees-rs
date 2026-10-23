#!/bin/sh
# Rebuilds the page of 3D test cases from the Payment Graph fixtures of utxo2webgraph-rs.
#
#   viz/build.sh            # from the repository root; PG_REFERENCE overrides the data
#
# The tree files land in tests/data (the fixtures of tests/space_payment_graph.rs) and in
# viz/out; the layouts come from `layout3` itself, checked for overlaps and against the
# flat engine; the page is viz/tidy3d.html, and a Node smoke test runs its logic.
set -eu
cd "$(dirname "$0")/.."

python3 viz/random_tree.py
python3 viz/pg_cases.py
python3 viz/pg_tenk.py
cargo run --quiet --release --example space_trees -- \
    tests/data/pg_tenk.tree tests/data/pg_funnel.tree tests/data/pg_first.tree viz/out/random.tree \
    > viz/out/layouts.json
python3 viz/assemble.py
node viz/harness.js viz/tidy3d.html

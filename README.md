# A Rust port

[![CI](https://github.com/massimo-nocentini/non-layered-tidy-trees-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/massimo-nocentini/non-layered-tidy-trees-rs/actions/workflows/ci.yml)
[![docs](https://img.shields.io/badge/docs-master-blue)](https://massimo-nocentini.github.io/non-layered-tidy-trees-rs/)
[![licence](https://img.shields.io/badge/licence-MIT-green)](LICENSE)

[`non-layered-tidy-trees.c`](https://github.com/massimo-nocentini/non-layered-tidy-trees.c) translated into plain Rust, with the `test/` suite translated
along with it, plus a second implementation of the same algorithm over struct-of-arrays
sweeps. The crate has no dependencies and no `unsafe` at all.

```sh
cargo test                        # the whole suite, ~8 s (the overlap sweep dominates)
cargo test --release              # the same, ~5 s
NLTT_TRIALS=25 cargo test         # a quicker sweep over the same shapes
cargo doc --open                  # the API, with the C names alongside

cargo run --release --bin bench   # the numbers below
make -C ../test bench                              # the C rows of the same table
```

The `Makefile` wraps the same commands — `make test` runs the suite, `make bench` prints
the tables below, `make doc` writes
the API into `docs/`, and `make help` lists the rest. [`CONTRIBUTING.md`](CONTRIBUTING.md)
says what a patch to a port has to keep true.

The port is intended to be *numerically* identical, not merely equivalent: the walks, the
sibling chain, the threading and the one behavioural fix in `separate` follow the C line by
line, in the same order, so the two produce bit-identical coordinates. That claim is
checked rather than asserted — see [the differential oracle](#the-differential-oracle),
which agrees on all 9 946 368 coordinates of the 115 200-tree corpus.

## The shape of the API

```rust
use non_layered_tidy_trees::{layout, Arena, LayoutInput};

let mut arena = Arena::new();
let root = arena.add_node(1, 30.0, 10.0, 0.0, false); // idx, w, h, margin, isdummy
let a = arena.add_node(2, 20.0, 10.0, 0.0, false);
let b = arena.add_node(3, 20.0, 30.0, 0.0, false);
arena.set_children(root, &[a, b]);

layout(&mut arena, &LayoutInput::new(root));

println!("{} {}", arena[root].x, arena[root].y);
```

| C | Rust |
|---|---|
| `tree_t` | `Node`, held by an `Arena` and named by a `NodeId` |
| `init_tree` / `free_tree` | `Arena::add_node` / dropping the arena |
| `t->c[i] = child` | `arena.set_children(t, &[child, ..])`, `arena.push_child(t, child)` |
| `treeinput_t` | `LayoutInput` (the data) plus `Callbacks` (the two hooks) |
| `layout(&input)` | `layout(&mut arena, &input)`, `layout_with(.., &mut cb)` |
| `walkcb` + `walkud` | `Callbacks::walk`, a `FnMut(&mut Arena, NodeId)` |
| `cpairscb` + `cpairsud` | `Callbacks::contour_pairs`, a `FnMut(&Arena, NodeId, NodeId, f64)` |
| `bottom(t, vertically)` | `arena.bottom(t, vertically)` |
| `layout_api`, `reifyflatchunks`, `flat_xy_into`, `flat_xywh_into` | same names, snake case |
| `maxbottom`, `maxbottombetween`, `fringemaxbottom_t` | `max_bottom`, `max_bottom_between`, `FringeMaxBottom` |
| `test/treegen.h` | the `treegen` module of the library |
| — | `flat::layout_flat`, `flat::Engine`, `flat::layout_api_flat`: the same algorithm over sweeps |

## What is not a literal translation

Everything in the "Differences with respect to the Java implementation" section of the
[root README](../README.md) still holds — horizontal layouting, centred coordinates, the
derived depth axis, per-node margins, the origin offset, the callbacks, the repeatability
of `layout`, the flat entry point. What changed *again*, going from C to Rust, is the shape
of the data rather than the arithmetic:

**Ids instead of pointers.** The threads (`tl`/`tr`), the extreme nodes (`el`/`er`) and the
parent links point at arbitrary nodes elsewhere in the tree, which is exactly the aliasing
that a tree of `Box`es cannot express. Nodes therefore live in an `Arena` and refer to each
other by index, and there is no `unsafe` in the crate.

**No `free_tree`.** Dropping the arena frees every node at once, without recursion — one
fewer stack-bounded walk than the C has.

**The sibling chain is a `Vec` used as a stack.** `update_chain` pops instead of freeing
links, and `separate` walks *down* the stack where the C follows `ih->nxt`. Running off the
end of the chain is a panic here rather than a null dereference; it cannot happen, for the
reason `tests/regression.rs::asymmetric_contours` pins down.

**Callbacks are closures, and they moved out of the input.** `void *walkud` and
`void *cpairsud` are captured state, so they have no counterpart; the hooks live in
`Callbacks` and `LayoutInput` is left as plain data that `layout` takes by shared
reference. The C test "`layout` does not write into its `treeinput_t`" therefore becomes a
property of the signature — it is still checked, in `tests/properties.rs::repeatability`.

**`vertically` and `centeredxy` are `bool`.** The C takes any nonzero `int`, and
`test/properties.c` checks that `2` and `-1` behave like `1`. That case cannot fail here
and has no port; what survives of it is the check that the flag reaches every node.

**`level` and `childno` are `usize`.** The C initializes them to `-1` and fills them in
during `setupWalk`; here they start at `0`. The only node that keeps the sentinel in C is
the root, whose `childno` `maxbottombetween` never reads, so the behaviour is the same.

**The generator is part of the library.** `test/treegen.h` becomes the `treegen` module,
because both the tests and the differential oracle need the very same corpus. Its PRNG is
bit-compatible with `tg_rand`, seeds included.

## The tests

Each C test file became one Rust test file, with the `PASS`/`FAIL` lines replaced by
assertions.

| C | Rust | What it pins down |
|---|---|---|
| `test/overlap.c` | `tests/overlap.rs` | no two node boxes overlap, over 4 regimes × depths 1–6 × 2–5 children × both orientations × centred/corner |
| `test/regression.c` | `tests/regression.rs` | the `first`-flag overlap in `separate`, and the shallow-left/deep-right contour shape |
| `test/properties.c` | `tests/properties.rs` | normalization, `x`/`y` as offsets, repeatability, centring as one rigid translation |
| `test/callbacks.c` | `tests/callbacks.rs` | the contour trace (compared against the one the C build prints), one `walk` call per node, a mutating callback that does not perturb normalization |
| `test/flatapi.c` | `tests/flatapi.rs` | `layout_api` and the three parallel array layouts |
| `test/depth.c` | `tests/depth.rs` | a chain of 10 000 levels; the walks run on a thread with a 64 MiB stack, since `cargo test` gives each test 2 MiB |
| — | `tests/flat.rs` | the sweeps against the recursion, bit for bit: 1 152 trees with their bookkeeping, a reused `Engine`, a 300 000-level chain, and `layout_api_flat` against `layout_api` |
| `overlap --dump` | `src/bin/dump.rs` | the differential oracle below |
| — | `tests/space.rs`, `tests/space_overlap.rs`, `tests/space_flat.rs`, `tests/space_payment_graph.rs` | the 3D layout of [`space`](#three-dimensions) |
| — | `src/lib.rs` unit tests | `max_bottom` / `max_bottom_between`, which the C suite does not cover; the expected values are the ones the C build prints |

`NLTT_TRIALS` sets the trees per shape (default 300, the C default) and `NLTT_DEPTH` the
depth of the chain (default 10 000).

## Tracing the phases

`NLTT_TRACE=1` makes every entry point narrate itself on standard error: one header line
for the call, then one line per phase with what it took and the little that is worth
knowing about it. Anything else — `0`, `no`, `off`, `false`, unset — is silence.

```sh
NLTT_TRACE=1 cargo run --example trace     # one small tree through every entry point
```

```text
[nltt] layout       root=#1 arena=4 vertically=true centeredxy=false origin=(0, 0) hooks=none
[nltt]   setup      875.000ns  nodes=4 depth=3
[nltt]   first        2.500µs
[nltt]   second     875.000ns  minbreadth=0
[nltt]   third       83.000ns  already at the origin
[nltt]   total        4.333µs
[nltt] layout_flat  root=#1 arena=4 vertically=true centeredxy=false origin=(0, 0)
[nltt]   build        4.917µs  nodes=4 depth=3
[nltt]   setup        1.416µs
[nltt]   first        2.833µs
[nltt]   second       2.792µs  minbreadth=0
[nltt]   third       42.000ns  already at the origin
[nltt]   write      833.000ns  4 nodes
[nltt]   total       12.833µs
```

The names are the ones the sources use: `setup`, `first`, `second` and `third` are the
walks — the sweeps, in `flat` — and `build`/`write` are the mirror being filled and read
back. `total` is the sum of the phases rather than the wall clock, so the trace's own
writes to standard error, which for a small tree outlast the layout, stay out of it.

The variable is read once per process and cached, so a traced build costs one load per
phase and nothing per node: the contour walk is not instrumented, and `NLTT_TRACE`
unset changes no timing that `make bench` can see. `Engine::profile` reports the same
phases as a value instead of a line, which is what `make bench-phases` tabulates.

## The mirror, halved

`layout_api` takes its tree as arrays and reifies an invisible "gap" node above every node
to space the levels apart, which doubles the largest thing in memory before the algorithm
has done anything. `flat` does not reify it: it becomes the upper *band* of the entry it
belongs to, so the mirror holds one entry per node with `bext`, `dext`, `bot`, `prelim` and
`modifier` per band and everything else — `el`, `er`, `msel`, `mser`, the threads, the
children, the parent — shared between the two.

This is a fold, not a merge, and the difference is the whole point. Folding the gap into
the node's own box (`h + gap`, one band) is *not* the same drawing: the gap node is a
narrower box in the general case, and it carries **no margin**, so a neighbouring subtree
may nestle into the band it occupies. Over 200 random trees from `treegen`, whose margins
are nonzero, a fold moves nodes in 177 of them. The banded entry keeps the gap node's own
width and its lack of a margin, and the contour walk still compares the two bands
separately, so the coordinates stay identical bit for bit —
`tests/flat.rs::the_paired_mirror_agrees_with_the_reified_gap_nodes` checks that against
the reified version over every gap-width convention a caller can pass.

Peak RSS of `layout_api_flat` over a 4 000 000-node tree, on an M-series laptop: **1481 MB
before, 1122 MB after**, and the sweeps run about 30% faster for having half the entries to
visit. The links are `usize` rather than `i32`/`u32` in the same breath: a mirror is now
bounded by the memory it fits in rather than by 2^31 nodes, where before it would have
truncated silently. That costs the contour walk a quarter of its time — it is memory bound,
and eight-byte links halve what a cache line holds — which the halved mirror pays back on
the API path and does not on the `Arena` path, where there are no gap nodes to fold.

## Three dimensions

`space` lays a tree out in three dimensions: every node is a box `w` × `h` × `d`, one axis
is the depth axis — `y` unless `LayoutInput3::depth` says otherwise — non-layered exactly as
above, and the children of a node are spread over the plane of the other two.

```rust
use non_layered_tidy_trees::space::{layout3, Arena3, AxisRule, LayoutInput3};

let mut arena = Arena3::new();
let root = arena.add_node(1, 10.0, 10.0, 10.0, 0.0); // idx, w, h, d, margin
let a = arena.add_node(2, 10.0, 10.0, 10.0, 0.0);
let b = arena.add_node(3, 10.0, 10.0, 10.0, 0.0);
arena.set_children(root, &[a, b]);

layout3(&mut arena, &LayoutInput3 { rule: AxisRule::Alternate, ..LayoutInput3::new(root) });
```

The 2D algorithm is linear because merging a subtree into its left siblings is one splice
of two threaded contours, and a splice needs the siblings in a line. So every internal node
picks a *family axis*, one of the two breadth axes — by `AxisRule` (`AllX`, `AllY`, `AllZ`,
`Alternate` by subtree height, or `Compact`, below) or by its own `axis` — and

* separates its children along that axis with the very same `separate`, sibling chain,
  `move_subtree` and `position_root`, over the contours along that axis;
* centres each child under its own box along the other axis.

Each subtree keeps four threaded contours, left and right along both breadth axes. Along
the family axis they are the 2D splice. Along the other axis the children are stacked in
depth rather than side by side, so the contour is the node, then one *hull* item covering
the depths down to the second deepest child with the extreme of all of them there, then a
thread into the deepest child's own contour. Either way a merge is a prefix plus one
thread, and the layout stays linear.

The entry points mirror the 2D ones:

| 2D | 3D |
|---|---|
| `layout`, `layout_with` | `space::layout3`, `space::layout3_with` |
| `vertically` | `LayoutInput3::depth`, any of `x`, `y`, `z` |
| `layout_api`, `reify_flat_chunks` | `space::layout_api3`, `space::reify3`: `whd` is `[w.., h.., d.., margin..]`, `gaps` the `w`, `h`, `d` of the gap nodes |
| `flat::layout_flat`, `flat::Engine`, `flat::layout_api_flat` | `space::flat::layout3_flat`, `space::flat::Engine3`, `space::flat::layout_api3_flat` |

The flat path is the same algorithm over a struct-of-arrays mirror, swept forwards and
backwards with no recursion at all, bit for bit the recursive coordinates. It is laid out
for the contour walk: what one step reads about a box is one 64-byte band, a cache line,
and what it reads about a node to find the next step is one link. From the arrays of the
API it folds the gap node above every node into the node's entry as its upper band, as the
2D mirror does. That is sound in 3D because a gap node has one child: its contours run
straight into the node below along both axes, and it never builds a hull.

`Compact` lays every family out along both breadth axes and keeps the one that gives the
subtree the shorter longest side, undoing the other. The first walk then does up to three
times its work and stays linear, since a trial only steps past contour items that the kept
axis's hull then hides. It is greedy, and not a better default than `Alternate`: it squares
every subtree, and a wide family then lines the squares up into a strip. The median longest
side, `Compact` over `Alternate`, 30 `bench_tree`s a cell:

| children per node | 100 | 1 000 | 10 000 | 100 000 |
|---|---:|---:|---:|---:|
| at most 2 | 0.89 | 0.84 | 0.75 | 0.76 |
| at most 4 | 0.78 | 0.84 | 1.04 | 1.22 |
| at most 8 | 0.91 | 0.88 | 1.72 | 1.94 |

It is worth it for narrow trees, and for small ones, not as a rule.

What it keeps, and what pins it down in `tests/space.rs`, `tests/space_overlap.rs`,
`tests/space_flat.rs` and `tests/space_payment_graph.rs`:

| | |
|---|---|
| no two boxes overlap, and margins hold along the separating axis | every pair, over the corpus of `tests/overlap.rs` with a `d` per box, under every rule and a random axis per node, plus boxes of zero extent |
| `AllX` is the 2D drawing: `x` and `y` bit for bit those of `layout`, whatever `d` is | the same corpus, both centring modes; `AllZ` likewise against `layout` with `d` for `w`; the depth along `x` with `AllY` against `layout` with `vertically` off; `layout_api3` against `layout_api` |
| another depth axis is a renaming of the axes | the depth along `x` and along `z` against the depth along `y` with the extents and axes swapped, bit for bit |
| the sweeps are the recursion | bit for bit, bookkeeping included, along every depth axis and under every rule; `layout_api3_flat` against `layout_api3` with unreached nodes; a chain of 300 000 levels on the default stack |
| parent centred over its first and last child, children in order, along the family axis; each child centred under its parent along the other | random trees, every rule |
| reversing every family mirrors the drawing along both breadth axes, without margins | random trees, every rule |
| isomorphic subtrees drawn alike | two copies of a subtree around a third |
| normalization, repeatability, `centered` as one rigid translation | as in `tests/properties.rs` |
| linear | 1.0–1.9 contour pairs per node from 2 000 to 32 000 nodes, on random, binary and spine shapes; 2.5–4.3 under `Compact` |
| real data | trees made from the Bitcoin Payment Graph in [`tests/data`](tests/data/README.md), the whole of the first 10 000 transactions among them: no overlap and the flat engine bit for bit under every rule, `AllX` against `layout`, every box starting where its parent's ends, and the 9 419 roots of the first 10 000 transactions packed into a square |
| the exact-arithmetic model of the design | `python3 prototype/xcheck.py`: 2 000 random trees within 1e-9 of `prototype/proto.py` with its axes given, and 2 000 with `Compact` choosing them against the prototype's own trial rule; see [`prototype/`](prototype/README.md) |

The price is compactness: siblings are separated along their family's axis even where they
are already apart along the other one, and a hull keeps a subtree's full extent at every
depth it covers.

[`viz/tidy3d.html`](viz/README.md) shows those trees, and the random one of the design
review, laid out by `layout3` under each rule. The Payment Graph ones take an output's
lifetime in blocks as the height of its box, which makes the non-layered depth axis exact
block height, and `√BTC` as its side. `viz/build.sh` rebuilds the fixtures, the layouts and
the page from a checkout of utxo2webgraph-rs.

`make bench` prints a second table for the 3D layout over the same trees, lifted with a `d`
per box. Its `-x` rows lay every family along the 2D breadth axis and carry the 2D checksum,
which they share with the `rust-rec` rows at every size. Best of 3, ns/node, `Alternate`
unless the row says `Compact`, depth along `y`, on the Xeon below:

| impl | 1 000 | 10 000 | 100 000 | 1 000 000 |
|---|---:|---:|---:|---:|
| `rust-rec` (2D, for scale) | 67.5 | 83.8 | 83.8 | 134.6 |
| `rust-3d` | 137.6 | 166.1 | 243.2 | 311.6 |
| `rust-3d-flat` | 157.3 | 188.0 | 291.3 | 425.1 |
| `rust-3d-flat-r` | 145.4 | 173.8 | 269.2 | **306.7** |
| `rust-3d-compact` (`Compact`) | 314.3 | 340.5 | 414.7 | 486.9 |
| `rust-3d-flat-r-c` (`Compact`) | 289.9 | 316.4 | 418.5 | 457.3 |
| `rust-api3` | 410.2 | 464.6 | 1161.7 | 1264.1 |
| `rust-api3-flat` | 204.1 | 225.9 | 287.5 | 600.4 |
| `rust-api3-flat-r` | 188.0 | 202.4 | 252.6 | **349.9** |

Over an `Arena3` the reused mirror now matches the recursion, where in 2D it stays behind:
the copy in and out costs what the cache-line layout saves, and the sweeps need no stack.
Over arrays there is no tree to build at all, and the sweeps are 2.2–4.6× faster than
reifying one. A `Node3` carries both breadth axes and two hulls, so every walk has about
twice the memory to go through of its 2D counterpart.

## Benchmarks

One tree of `n` nodes, laid out repeatedly; the tree is built outside the timed region and
the first layout is a warm-up, so what is measured is `layout` itself. `test/bench.c` and
`src/bin/bench.rs` build the *same* trees — the two generators draw from the same PRNG in
the same order — and both print a checksum that is the XOR of every coordinate's bit
pattern. Every row below carries the same checksum for a given size: these are all the same
drawing, computed five different ways.

Xeon Gold 6238R at 2.2 GHz, clang 18 `-O2` (gcc 13 is within 5%), rustc 1.97 release,
ns/node, best of 7, averaged over both orientations. `-r` marks the rows that reuse their
mirror across calls.

> These rows predate two changes to the mirror — `usize` links and the paired entries
> described in [the mirror, halved](#the-mirror-halved) — which move the `rust-flat` rows up
> by about a quarter and the `rust-api-flat` rows down by about a third. Rerun `make bench`
> before quoting them.

**`layout`, over a tree already in memory:**

| impl | 1 000 | 10 000 | 100 000 | 1 000 000 |
|---|---:|---:|---:|---:|
| `c` | 56.5 | 69.8 | 70.3 | **106.4** |
| `rust-rec` (the port) | 69.3 | 83.1 | 84.8 | 135.5 |
| `rust-flat` | 67.1 | 86.0 | 107.7 | 210.1 |
| `rust-flat-r` | 56.6 | 81.5 | 101.2 | 171.4 |

**`layout_api`, arrays in and arrays out:**

| impl | 1 000 | 10 000 | 100 000 | 1 000 000 |
|---|---:|---:|---:|---:|
| `c-api` | 254.3 | 283.2 | 367.9 | 548.0 |
| `rust-api` (the port) | 235.9 | 264.6 | 497.2 | 657.1 |
| `rust-api-flat` | 130.5 | 143.6 | 166.2 | 356.7 |
| `rust-api-flat-r` | 120.0 | 137.0 | 152.5 | **256.9** |

### What the numbers say

**Explicit SIMD was worth nothing here, and is gone.** The crate used to carry hand-written
AVX2 kernels behind a `simd` feature — bit-identical to the scalar loops, and the same speed
to within noise in both tables, at every size. The phase breakdown says why
(`bench --phases`, 1 M nodes, µs):

| | build | setup | first | second | third | write |
|---|---:|---:|---:|---:|---:|---:|
| scalar | 67 895 | 5 690 | 57 536 | 9 421 | 1 147 | 26 956 |
| AVX2 | 67 468 | 6 258 | 56 302 | 9 033 | 1 075 | 27 215 |

The three vectorizable sweeps are `setup`, `second` and `third`: 16.3 ms of 168.6 ms, under
10% of the runtime — and they are streaming over arrays at roughly 13 GB/s, which is memory
bandwidth, not arithmetic. Four lanes of a bandwidth-bound loop are still one memory system.
`first` — the contour walk, the algorithm proper — is a third of the time on its own and
cannot be vectorized at any width. Those loops are now left plain, for the compiler to widen
as it sees fit, and the crate has no `unsafe` left anywhere.

**The restructuring is what pays, and only when the input is already flat.** Over an
`Arena` the mirror is a loss: `build` plus `write` is 56% of a flat layout, which is more
than the pointer chasing it removes, because a Rust `Arena` is already one contiguous
`Vec<Node>` — the locality a struct-of-arrays rewrite chases in the C is something the port
had from the start. Over `layout_api` there is no tree to mirror in the first
place, the 2n nodes and their child lists never get allocated, and the same code is **2.1×
faster than the C** and 2.6× faster than the pointer-based Rust.

**The C is still the one to beat for `layout`.** The recursive port is ~25% behind it, which
is where bounds-checked indexing in the contour walk shows up; the C `tree_t` reaches a
sibling through one pointer where `Arena` indexes a `Vec` and then a `Vec` of children.

## The differential oracle

`test/overlap.c --dump` prints the corpus instead of checking it. `src/bin/dump.rs` does
the same, and `--check` compares against a dump instead of diffing it — C's `%.17g` and
Rust's shortest round-trip formatting spell the same `f64` differently, but both round-trip,
so parsing makes the comparison exact:

```sh
make -C ../test overlap
../test/overlap --dump 300 > golden.txt              # 62 MB, the C coordinates
cargo run --release --bin dump -- --check golden.txt 300
# PASS  9946368 coordinates over 115200 trees, recursive identical to golden.txt

cargo run --release --bin dump -- --flat --check golden.txt 300
```

Both paths pass: the sweeps reproduce the C build's coordinates exactly, not approximately.

Use a smaller trial count (`--dump 10`, `--check golden.txt 10`) for a couple of MB.

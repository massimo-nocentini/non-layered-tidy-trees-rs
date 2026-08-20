//! The sweeps against the recursion.
//!
//! `flat` computes the same drawing as `layout` by a different route: a struct-of-arrays
//! mirror in breadth-first order, swept forwards and backwards instead of recursed over.
//! The claim it has to earn is that "the same drawing" means *bit for bit*, not "to within
//! a tolerance" — every assertion here is an exact comparison.
//!
//! `src/bin/dump.rs --flat --check` makes the same comparison against the C build itself,
//! over the whole corpus of `test/overlap.c`.

use non_layered_tidy_trees::flat::{self, Engine};
use non_layered_tidy_trees::treegen::{self, Regime, REGIMES};
use non_layered_tidy_trees::{layout, layout_api, Arena, LayoutInput};

/// Every tree of the sweep, laid out both ways.
#[test]
fn the_sweeps_agree_with_the_recursion() {
    let mut engine = Engine::new();
    let mut checked = 0;

    for regime in REGIMES {
        for maxdepth in 1..=6 {
            for maxkids in 2..=4 {
                for vert in [false, true] {
                    for cent in [false, true] {
                        for k in 0..12u64 {
                            let seed = 31 + k * 977 + (maxdepth * 10 + maxkids) as u64;

                            let (mut a, root_a) = treegen::build(seed, maxdepth, maxkids, regime);
                            let input = LayoutInput {
                                root: root_a,
                                vertically: vert,
                                centeredxy: cent,
                                x: 3.5,
                                y: -2.25,
                            };
                            layout(&mut a, &input);

                            let (mut b, root_b) = treegen::build(seed, maxdepth, maxkids, regime);
                            let input_b = LayoutInput {
                                root: root_b,
                                ..input
                            };
                            engine.layout(&mut b, &input_b);

                            let (ra, rb) =
                                (treegen::collect(&a, root_a), treegen::collect(&b, root_b));
                            assert_eq!(ra.len(), rb.len());

                            for (&x, &y) in ra.iter().zip(&rb) {
                                assert_eq!(
                                    (a[x].x.to_bits(), a[x].y.to_bits()),
                                    (b[y].x.to_bits(), b[y].y.to_bits()),
                                    "node {} differs (regime {}, depth {maxdepth}, kids {maxkids}, \
                                     vert {vert}, cent {cent}, seed {seed})",
                                    a[x].idx,
                                    regime.index()
                                );

                                /* the bookkeeping has to survive the detour too */
                                assert_eq!(a[x].level, b[y].level, "level of node {}", a[x].idx);
                                assert_eq!(
                                    a[x].childno, b[y].childno,
                                    "childno of node {}",
                                    a[x].idx
                                );
                                assert_eq!(
                                    a[x].centeredxy, b[y].centeredxy,
                                    "centeredxy of node {}",
                                    a[x].idx
                                );
                                assert_eq!(
                                    a[x].parent.map(|p| a[p].idx),
                                    b[y].parent.map(|p| b[p].idx),
                                    "parent of node {}",
                                    a[x].idx
                                );
                            }

                            checked += 1;
                        }
                    }
                }
            }
        }
    }

    assert!(checked > 1000, "only {checked} trees were compared");
}

/// The engine reuses its mirror; the second layout has to be the first one again.
#[test]
fn a_reused_engine_does_not_drift() {
    let mut engine = Engine::new();

    let (mut arena, root) = treegen::build(4242, 5, 4, Regime::Varied);
    let input = LayoutInput::new(root);

    engine.layout(&mut arena, &input);
    let first: Vec<(u64, u64)> = treegen::collect(&arena, root)
        .iter()
        .map(|&id| (arena[id].x.to_bits(), arena[id].y.to_bits()))
        .collect();

    /* another tree in between, to make sure nothing of it is left behind */
    let (mut other, other_root) = treegen::build(7, 4, 3, Regime::Tiny);
    engine.layout(&mut other, &LayoutInput::new(other_root));

    engine.layout(&mut arena, &input);
    engine.layout(&mut arena, &input);

    let again: Vec<(u64, u64)> = treegen::collect(&arena, root)
        .iter()
        .map(|&id| (arena[id].x.to_bits(), arena[id].y.to_bits()))
        .collect();

    assert_eq!(first, again, "a reused engine drifted");
}

/// A tree that is one long chain: the sweeps do not recurse, so no stack is involved.
#[test]
fn a_chain_far_deeper_than_the_stack_allows() {
    let depth = 300_000;

    let mut arena = Arena::with_capacity(depth);
    let root = treegen::chain(&mut arena, 1, depth, 10.0, 10.0);

    flat::layout_flat(&mut arena, &LayoutInput::new(root));

    let mut last = root;
    while let Some(&c) = arena.children(last).first() {
        last = c;
    }

    assert_eq!(arena[last].y, 10.0 * (depth - 1) as f64);
    assert_eq!(arena[last].level, depth - 1);
}

/// The flat API, both ways round.
#[test]
fn layout_api_flat_agrees_with_layout_api() {
    for (n, children, rooti) in [
        (3usize, vec![2usize, 0, 0, 2, 3], 1usize),
        (1, vec![0], 1),
        (7, vec![2, 2, 2, 0, 0, 0, 0, 2, 3, 4, 5, 6, 7], 1),
        /* the root is not node 1, and node 4 is unreachable */
        (5, vec![2, 1, 0, 0, 0, 3, 5, 2], 1),
    ] {
        for vert in [false, true] {
            for cent in [false, true] {
                // the last `n` are the margins, which is what tells the gap node's band
                // apart from the node's own -- see the paired mirror in `flat`.
                let mut wh: Vec<f64> = (0..3 * n)
                    .map(|i| {
                        if i < 2 * n {
                            10.0 + (i % 5) as f64
                        } else {
                            (i % 3) as f64
                        }
                    })
                    .collect();
                let whg: Vec<f64> = (0..2 * n).map(|i| 5.0 + (i % 3) as f64).collect();

                let mut expected = wh.clone();
                layout_api(
                    n,
                    &mut expected,
                    &whg,
                    &children,
                    rooti,
                    vert,
                    cent,
                    1.5,
                    -0.5,
                );

                flat::layout_api_flat(n, &mut wh, &whg, &children, rooti, vert, cent, 1.5, -0.5);

                for i in 0..2 * n {
                    assert_eq!(
                        wh[i].to_bits(),
                        expected[i].to_bits(),
                        "slot {i} of {n} nodes (vert {vert}, cent {cent}): {} vs {}",
                        wh[i],
                        expected[i]
                    );
                }
            }
        }
    }
}

/// The paired mirror against the gap nodes the recursion actually builds.
///
/// `layout_api` reifies an invisible node above every node to space the levels apart;
/// `flat` folds it into the node's own entry as a second band. That fold is only exact
/// because the band keeps the gap node's *width* and its lack of a margin: giving it the
/// node's margin instead moves nodes, which is what this sweep would catch.
///
/// The trees come from `bench_arrays`, whose margins are nonzero, under all three
/// gap-width regimes a caller can pass.
#[test]
fn the_paired_mirror_agrees_with_the_reified_gap_nodes() {
    let trials: u64 = std::env::var("NLTT_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);

    /// A single node, a pair, a fan, and enough of a tree to thread through.
    const SIZES: [usize; 7] = [1, 2, 3, 7, 40, 300, 1500];

    let mut checked = 0;

    for seed in 0..trials {
        for &n in &SIZES {
            let (wh, mut whg, children) = treegen::bench_arrays(n, 5, seed);

            match seed % 3 {
                // the gap node as wide as its node, which is what `bench_arrays` builds
                0 => {}
                // a gap node of no width at all, the other convention
                1 => (0..n).for_each(|i| whg[i] = 0.0),
                // and one that is neither
                _ => (0..n).for_each(|i| whg[i] = wh[i] * 0.5 + (i % 7) as f64),
            }

            for vert in [false, true] {
                for cent in [false, true] {
                    let mut expected = wh.clone();
                    let mut got = wh.clone();

                    layout_api(n, &mut expected, &whg, &children, 1, vert, cent, 1.5, -0.5);
                    flat::layout_api_flat(n, &mut got, &whg, &children, 1, vert, cent, 1.5, -0.5);

                    for i in 0..2 * n {
                        assert_eq!(
                            got[i].to_bits(),
                            expected[i].to_bits(),
                            "n={n} seed={seed} vert={vert} cent={cent}, value {i}: \
                             {} against {}",
                            got[i],
                            expected[i]
                        );
                    }

                    checked += 2 * n;
                }
            }
        }
    }

    // `x` and `y` of every node, over both orientations and both centrings
    let expected = trials as usize * 4 * 2 * SIZES.iter().sum::<usize>();
    assert_eq!(checked, expected, "the sweep did not run over every tree");
}

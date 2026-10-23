//! The other ways into the 3D layout, against `layout3` and against the 2D layout.
//!
//! * **sweeps** — `space::flat` agrees with the recursion bit for bit, bookkeeping included,
//!   along every depth axis and under every rule
//! * **depth axes** — laying a tree out along `x` or `z` is laying it out along `y` with the
//!   axes renamed, bit for bit; along `x` with every family along `y` it is the 2D drawing
//!   with `vertically` off
//! * **arrays** — `layout_api3` is `layout_api` along `x`, and `layout_api3_flat` is
//!   `layout_api3`, unreached nodes included
//! * **depth** — the sweeps lay out a chain of 300 000 levels on the default stack
//!
//! Every comparison here is exact.

use non_layered_tidy_trees::space::flat::{layout_api3_flat, Engine3};
use non_layered_tidy_trees::space::{
    layout3, layout_api3, ApiInput3, Arena3, Axis, AxisRule, LayoutInput3,
};
use non_layered_tidy_trees::treegen::{self, Regime, Rng, REGIMES};
use non_layered_tidy_trees::{layout, layout_api, LayoutInput, NodeId};

/// A tree of the 2D corpus with a random `d` per box, and a random axis on some nodes.
fn tree(
    seed: u64,
    maxdepth: usize,
    maxkids: usize,
    regime: Regime,
    axes: &[Axis],
) -> (Arena3, NodeId) {
    let (arena, root) = treegen::build(seed, maxdepth, maxkids, regime);
    let mut rng = Rng::new(seed ^ 0x5DEE_CE66);
    let (mut a, root) = Arena3::lift(&arena, root, |_| (1 + rng.rand(40)) as f64);
    if !axes.is_empty() {
        for v in a.preorder(root) {
            if rng.rand(3) == 0 {
                a[v].axis = Some(axes[rng.rand(axes.len() as u64) as usize]);
            }
        }
    }
    (a, root)
}

/// The breadth axes of a depth axis, and the rules that can go with it.
fn frame(depth: Axis) -> ([Axis; 2], [AxisRule; 3]) {
    match depth {
        Axis::X => (
            [Axis::Y, Axis::Z],
            [AxisRule::AllY, AxisRule::AllZ, AxisRule::Alternate],
        ),
        Axis::Y => (
            [Axis::X, Axis::Z],
            [AxisRule::AllX, AxisRule::AllZ, AxisRule::Alternate],
        ),
        Axis::Z => (
            [Axis::X, Axis::Y],
            [AxisRule::AllX, AxisRule::AllY, AxisRule::Alternate],
        ),
    }
}

fn bits(a: &Arena3, v: NodeId) -> [u64; 3] {
    [a[v].x.to_bits(), a[v].y.to_bits(), a[v].z.to_bits()]
}

#[test]
fn the_sweeps_agree_with_the_recursion() {
    let mut engine = Engine3::new();
    let mut checked = 0;

    for regime in REGIMES {
        for maxdepth in 1..=6 {
            for maxkids in 2..=4 {
                for depth in [Axis::X, Axis::Y, Axis::Z] {
                    let (breadth, rules) = frame(depth);
                    for rule in rules {
                        for cent in [false, true] {
                            for k in 0..4u64 {
                                let seed = 31 + k * 977 + (maxdepth * 10 + maxkids) as u64;
                                let axes = if k % 2 == 0 { &breadth[..] } else { &[] };
                                let (mut a, root) = tree(seed, maxdepth, maxkids, regime, axes);
                                let (mut b, _) = tree(seed, maxdepth, maxkids, regime, axes);
                                let input = LayoutInput3 {
                                    root,
                                    depth,
                                    centered: cent,
                                    rule,
                                    x: 3.5,
                                    y: -2.25,
                                    z: 0.125,
                                };

                                layout3(&mut a, &input);
                                engine.layout(&mut b, &input);

                                for v in a.preorder(root) {
                                    let (p, q) = (&a[v], &b[v]);
                                    assert_eq!(
                                        bits(&a, v),
                                        bits(&b, v),
                                        "#{} differs (regime {}, depth {maxdepth}, kids {maxkids}, \
                                         {depth:?} {rule:?}, cent {cent}, seed {seed})",
                                        p.idx,
                                        regime.index()
                                    );
                                    assert_eq!(
                                        (p.level, p.childno, p.parent, p.family(), p.centered),
                                        (q.level, q.childno, q.parent, q.family(), q.centered),
                                        "the bookkeeping of #{} differs",
                                        p.idx
                                    );
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(checked > 0);
}

/// Renames the axes of a tree: its extents, the axes its nodes ask for.
fn renamed(a: &Arena3, root: NodeId, swap: (Axis, Axis)) -> Arena3 {
    let rename = |ax: Axis| {
        if ax == swap.0 {
            swap.1
        } else if ax == swap.1 {
            swap.0
        } else {
            ax
        }
    };
    let mut b = a.clone();
    for v in a.preorder(root) {
        let n = &a[v];
        let e = [n.w, n.h, n.d];
        let m = &mut b[v];
        m.w = e[rename(Axis::X).index()];
        m.h = e[rename(Axis::Y).index()];
        m.d = e[rename(Axis::Z).index()];
        m.axis = n.axis.map(rename);
    }
    b
}

#[test]
fn a_depth_axis_is_a_renaming() {
    let mut differ = 0;
    let mut trees = 0;

    for (depth, swap, rules) in [
        (
            Axis::X,
            (Axis::X, Axis::Y),
            [
                (AxisRule::AllX, AxisRule::AllY),
                (AxisRule::AllZ, AxisRule::AllZ),
                (AxisRule::Alternate, AxisRule::Alternate),
            ],
        ),
        (
            Axis::Z,
            (Axis::Y, Axis::Z),
            [
                (AxisRule::AllX, AxisRule::AllX),
                (AxisRule::AllZ, AxisRule::AllY),
                (AxisRule::Alternate, AxisRule::Alternate),
            ],
        ),
    ] {
        for (rule_y, rule_d) in rules {
            for regime in REGIMES {
                for cent in [false, true] {
                    for k in 0..40u64 {
                        let (mut a, root) = tree(600 + k, 5, 4, regime, &[Axis::X, Axis::Z]);
                        let mut b = renamed(&a, root, swap);

                        layout3(
                            &mut a,
                            &LayoutInput3 {
                                centered: cent,
                                rule: rule_y,
                                x: 1.0,
                                y: 2.0,
                                z: 3.0,
                                ..LayoutInput3::new(root)
                            },
                        );
                        let o = [1.0, 2.0, 3.0];
                        let (i, j) = (swap.0.index(), swap.1.index());
                        let mut ob = o;
                        ob.swap(i, j);
                        layout3(
                            &mut b,
                            &LayoutInput3 {
                                depth,
                                centered: cent,
                                rule: rule_d,
                                x: ob[0],
                                y: ob[1],
                                z: ob[2],
                                ..LayoutInput3::new(root)
                            },
                        );

                        trees += 1;
                        for v in a.preorder(root) {
                            let mut p = bits(&a, v);
                            p.swap(i, j);
                            if p != bits(&b, v) {
                                differ += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        differ, 0,
        "{differ} nodes over {trees} trees move under a renaming of the axes"
    );
}

#[test]
fn along_x_with_families_along_y_is_the_horizontal_2d_drawing() {
    let mut bad = 0;
    let mut nodes = 0;

    for regime in REGIMES {
        for cent in [false, true] {
            for k in 0..100u64 {
                let (mut flat, root) = treegen::build(77 + k, 5, 4, regime);
                let input = LayoutInput {
                    vertically: false,
                    centeredxy: cent,
                    x: -1.5,
                    y: 4.0,
                    ..LayoutInput::new(root)
                };
                layout(&mut flat, &input);

                let (mut a, r) = tree(77 + k, 5, 4, regime, &[]);
                layout3(
                    &mut a,
                    &LayoutInput3 {
                        depth: Axis::X,
                        rule: AxisRule::AllY,
                        centered: cent,
                        x: -1.5,
                        y: 4.0,
                        ..LayoutInput3::new(r)
                    },
                );

                for (p, q) in flat.preorder(root).into_iter().zip(a.preorder(r)) {
                    nodes += 1;
                    if flat[p].x.to_bits() != a[q].x.to_bits()
                        || flat[p].y.to_bits() != a[q].y.to_bits()
                    {
                        bad += 1;
                    }
                }
            }
        }
    }

    assert_eq!(
        bad, 0,
        "{bad} of {nodes} nodes differ from `layout` with `vertically` off"
    );
}

/// The arrays of `bench_arrays`, with a `d` for every node and every gap node.
fn arrays(n: usize, seed: u64) -> (Vec<f64>, Vec<f64>, Vec<usize>) {
    let (wh, whg, children) = treegen::bench_arrays(n, 4, seed);
    let mut rng = Rng::new(seed);
    let mut whd = vec![0.0; 4 * n];
    whd[..2 * n].copy_from_slice(&wh[..2 * n]);
    for i in 0..n {
        whd[2 * n + i] = (1 + rng.rand(40)) as f64;
    }
    whd[3 * n..].copy_from_slice(&wh[2 * n..3 * n]);

    let mut gaps = vec![0.0; 3 * n];
    gaps[..2 * n].copy_from_slice(&whg);
    for i in 0..n {
        gaps[2 * n + i] = rng.rand(5) as f64;
    }
    (whd, gaps, children)
}

#[test]
fn the_api_along_x_is_the_2d_api() {
    for n in [1usize, 2, 7, 100, 3000] {
        for (vertically, depth, rule) in [
            (true, Axis::Y, AxisRule::AllX),
            (false, Axis::X, AxisRule::AllY),
        ] {
            for cent in [false, true] {
                let (mut whd, gaps, children) = arrays(n, n as u64);
                let (mut wh, whg, _) = treegen::bench_arrays(n, 4, n as u64);

                layout_api(n, &mut wh, &whg, &children, 1, vertically, cent, 2.0, -3.0);
                let input = ApiInput3 {
                    depth,
                    centered: cent,
                    rule,
                    x: 2.0,
                    y: -3.0,
                    z: 0.0,
                };
                layout_api3(n, &mut whd, &gaps, &children, 1, &input);

                for i in 0..2 * n {
                    assert_eq!(
                        wh[i].to_bits(),
                        whd[i].to_bits(),
                        "n={n} {depth:?} cent={cent}: value {i} differs"
                    );
                }
            }
        }
    }
}

#[test]
fn the_flat_api_agrees_with_the_api() {
    let mut engine = Engine3::new();

    for n in [1usize, 2, 7, 100, 3000] {
        for depth in [Axis::X, Axis::Y, Axis::Z] {
            for rule in frame(depth).1 {
                for cent in [false, true] {
                    for rooti in [1, n.div_ceil(2)] {
                        let (mut a, gaps, children) = arrays(n, 5 + n as u64);
                        let mut b = a.clone();
                        let mut c = a.clone();
                        let input = ApiInput3 {
                            depth,
                            centered: cent,
                            rule,
                            x: 0.5,
                            y: 1.5,
                            z: -2.5,
                        };

                        layout_api3(n, &mut a, &gaps, &children, rooti, &input);
                        layout_api3_flat(n, &mut b, &gaps, &children, rooti, &input);
                        engine.layout_api(n, &mut c, &gaps, &children, rooti, &input);

                        for i in 0..3 * n {
                            assert_eq!(
                                a[i].to_bits(),
                                b[i].to_bits(),
                                "n={n} root={rooti} {depth:?} {rule:?} cent={cent}: value {i}"
                            );
                            assert_eq!(
                                a[i].to_bits(),
                                c[i].to_bits(),
                                "the reused engine differs at value {i}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_sweeps_need_no_stack() {
    let depth = 300_000;
    let mut a = Arena3::with_capacity(depth);
    let root = a.add_node(1, 10.0, 10.0, 10.0, 0.0);
    let mut last = root;
    for i in 1..depth {
        let n = a.add_node(i + 1, 10.0, 10.0, 10.0, 0.0);
        a.push_child(last, n);
        last = n;
    }

    Engine3::new().layout(
        &mut a,
        &LayoutInput3 {
            rule: AxisRule::Alternate,
            ..LayoutInput3::new(root)
        },
    );

    assert_eq!(
        (a[last].x, a[last].y, a[last].z),
        (0.0, 10.0 * (depth - 1) as f64, 0.0)
    );
    assert_eq!(a[last].level, depth - 1);
}

#[test]
#[should_panic(expected = "which is the depth axis")]
fn a_family_along_the_depth_axis_is_refused() {
    let mut a = Arena3::new();
    let root = a.add_node(1, 1.0, 1.0, 1.0, 0.0);
    let leaf = a.add_node(2, 1.0, 1.0, 1.0, 0.0);
    a.set_children(root, &[leaf]);
    layout3(
        &mut a,
        &LayoutInput3 {
            rule: AxisRule::AllY,
            ..LayoutInput3::new(root)
        },
    );
}

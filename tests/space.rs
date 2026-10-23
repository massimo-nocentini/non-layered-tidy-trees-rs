//! The 3D layout of `space`: what it keeps of the 2D one, and what it adds.
//!
//! * **reduction** — with every family along `x`, `x` and `y` are bit for bit those of
//!   `layout`; along `z`, `z` is what `layout` draws with `d` for `w`
//! * **normalization**, **repeatability** and **centering**, as in `tests/properties.rs`
//! * **tidiness** — each parent centred over its first and last child along its family
//!   axis, each child centred under its parent along the other, children in order
//! * **symmetry** — reversing every family mirrors the drawing along both axes
//! * **congruence** — isomorphic subtrees are drawn alike, up to a translation
//! * **linearity** — the contour pairs compared stay within a constant per node
//! * **depth** — a chain of 10 000 levels, on a thread with a large stack

use non_layered_tidy_trees::space::{
    layout3, layout3_with, Arena3, Axis, AxisRule, Callbacks3, Item, LayoutInput3,
};
use non_layered_tidy_trees::treegen::{self, Regime, Rng, REGIMES};
use non_layered_tidy_trees::{layout, Arena, LayoutInput, NodeId};

const RULES: [AxisRule; 3] = [AxisRule::AllX, AxisRule::AllZ, AxisRule::Alternate];

fn close_enough(a: f64, b: f64) -> bool {
    (a - b) < 1e-9 && (b - a) < 1e-9
}

/// A tree of the 2D corpus with a random `d` per box.
fn tree(seed: u64, maxdepth: usize, maxkids: usize, regime: Regime) -> (Arena3, NodeId) {
    let (arena, root) = treegen::build(seed, maxdepth, maxkids, regime);
    let mut rng = Rng::new(seed ^ 0x5DEE_CE66);
    Arena3::lift(&arena, root, |_| (1 + rng.rand(40)) as f64)
}

/// The smallest corner of a box.
fn lo(a: &Arena3, t: NodeId) -> [f64; 3] {
    let n = &a[t];
    if n.centered {
        [n.x - n.w / 2.0, n.y - n.h / 2.0, n.z - n.d / 2.0]
    } else {
        [n.x, n.y, n.z]
    }
}

fn breadth(a: &Arena3, t: NodeId, ax: Axis) -> f64 {
    lo(a, t)[ax.index()]
}

/// The other breadth axis, the depth being along `y`.
fn cross(ax: Axis) -> Axis {
    if ax == Axis::X {
        Axis::Z
    } else {
        Axis::X
    }
}

#[test]
fn reduction_to_2d() {
    let mut xbad = 0;
    let mut zbad = 0;
    let mut nodes = 0;

    for regime in REGIMES {
        for maxdepth in 1..=5 {
            for maxkids in 2..=5 {
                for cent in [false, true] {
                    for k in 0..40u64 {
                        let seed = 31 * k + (regime.index() * 100 + maxdepth * 10 + maxkids) as u64;

                        let (mut flat, root) = treegen::build(seed, maxdepth, maxkids, regime);
                        let input = LayoutInput {
                            centeredxy: cent,
                            ..LayoutInput::new(root)
                        };
                        layout(&mut flat, &input);
                        let order = flat.preorder(root);

                        // along x, with an arbitrary d
                        let (mut a, r) = tree(seed, maxdepth, maxkids, regime);
                        let along = |rule| LayoutInput3 {
                            centered: cent,
                            rule,
                            ..LayoutInput3::new(r)
                        };
                        layout3(&mut a, &along(AxisRule::AllX));
                        for (&p, q) in order.iter().zip(a.preorder(r)) {
                            if flat[p].x.to_bits() != a[q].x.to_bits()
                                || flat[p].y.to_bits() != a[q].y.to_bits()
                            {
                                xbad += 1;
                            }
                        }

                        // along z, with w and d swapped
                        for q in a.preorder(r) {
                            let n = &mut a[q];
                            (n.w, n.d) = (n.d, n.w);
                        }
                        layout3(&mut a, &along(AxisRule::AllZ));
                        for (&p, q) in order.iter().zip(a.preorder(r)) {
                            if flat[p].x.to_bits() != a[q].z.to_bits()
                                || flat[p].y.to_bits() != a[q].y.to_bits()
                            {
                                zbad += 1;
                            }
                        }

                        nodes += order.len();
                    }
                }
            }
        }
    }

    assert_eq!(
        xbad, 0,
        "AllX differs from `layout` at {xbad} of {nodes} nodes"
    );
    assert_eq!(
        zbad, 0,
        "AllZ differs from `layout` at {zbad} of {nodes} nodes"
    );
}

#[test]
fn normalization() {
    let mut bad = 0;
    let mut total = 0;

    for regime in REGIMES {
        for rule in RULES {
            for cent in [false, true] {
                for k in 0..60u64 {
                    for (x, y, z) in [(0.0, 0.0, 0.0), (100.0, 7.0, -3.5)] {
                        let (mut a, root) = tree(4242 + k, 4, 3, regime);
                        let input = LayoutInput3 {
                            root,
                            depth: Axis::Y,
                            centered: cent,
                            rule,
                            x,
                            y,
                            z,
                        };
                        layout3(&mut a, &input);

                        let nodes = a.preorder(root);
                        let minx = nodes.iter().map(|&v| a[v].x).fold(f64::INFINITY, f64::min);
                        let minz = nodes.iter().map(|&v| a[v].z).fold(f64::INFINITY, f64::min);
                        total += 1;
                        if !close_enough(minx, x)
                            || !close_enough(minz, z)
                            || !close_enough(lo(&a, root)[1], y)
                        {
                            bad += 1;
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        bad, 0,
        "the drawing does not start at the requested origin in {bad} of {total} layouts"
    );
}

#[test]
fn repeatability() {
    for rule in RULES {
        let (mut a, root) = tree(99, 5, 4, Regime::Varied);
        let input = LayoutInput3 {
            rule,
            ..LayoutInput3::new(root)
        };

        layout3(&mut a, &input);
        let nodes = a.preorder(root);
        let first: Vec<_> = nodes.iter().map(|&v| (a[v].x, a[v].y, a[v].z)).collect();

        let before = input;
        layout3(&mut a, &input);
        layout3(&mut a, &input);

        let drift = nodes
            .iter()
            .zip(&first)
            .filter(|(&v, &(x, y, z))| a[v].x != x || a[v].y != y || a[v].z != z)
            .count();

        assert_eq!(
            drift,
            0,
            "{rule:?}: repeated layouts moved {drift} of {} nodes",
            nodes.len()
        );
        assert_eq!(before, input, "`layout3` wrote into its `LayoutInput3`");
    }
}

/// `centered` moves the coordinates to the middle of the boxes and the drawing by one
/// rigid translation, as in 2D.
#[test]
fn centering() {
    let mut mismatch = 0;

    for rule in RULES {
        for k in 0..100u64 {
            let (mut a, root) = tree(31 + k, 4, 3, Regime::Varied);
            layout3(
                &mut a,
                &LayoutInput3 {
                    rule,
                    ..LayoutInput3::new(root)
                },
            );
            let (mut b, _) = tree(31 + k, 4, 3, Regime::Varied);
            layout3(
                &mut b,
                &LayoutInput3 {
                    rule,
                    centered: true,
                    ..LayoutInput3::new(root)
                },
            );

            let (la, lb) = (lo(&a, root), lo(&b, root));
            for v in a.preorder(root) {
                let (pa, pb) = (lo(&a, v), lo(&b, v));
                if (0..3).any(|q| !close_enough(pb[q] - pa[q], lb[q] - la[q])) || !b[v].centered {
                    mismatch += 1;
                }
            }
        }
    }

    assert_eq!(
        mismatch, 0,
        "`centered` moved {mismatch} nodes beyond one rigid translation"
    );
}

/// Along the family axis a parent is centred over its first and last child and the
/// children come in order; along the other axis each child is centred under the parent.
#[test]
fn tidiness() {
    let (mut uncentred, mut unordered, mut offcross) = (0, 0, 0);

    for rule in RULES {
        for k in 0..100u64 {
            let (mut a, root) = tree(500 + k, 5, 4, Regime::Varied);
            layout3(
                &mut a,
                &LayoutInput3 {
                    rule,
                    ..LayoutInput3::new(root)
                },
            );

            for v in a.preorder(root) {
                let cs = a.children(v).to_vec();
                if cs.is_empty() {
                    continue;
                }
                let (fa, cr) = (a[v].family(), cross(a[v].family()));
                let (f, l) = (cs[0], cs[cs.len() - 1]);

                let mid = breadth(&a, v, fa) + a[v].extent(fa) / 2.0;
                let span = (breadth(&a, f, fa) + breadth(&a, l, fa) + a[l].extent(fa)) / 2.0;
                if !close_enough(mid, span) {
                    uncentred += 1;
                }

                for w in cs.windows(2) {
                    if breadth(&a, w[1], fa) < breadth(&a, w[0], fa) {
                        unordered += 1;
                    }
                }

                let vmid = breadth(&a, v, cr) + a[v].extent(cr) / 2.0;
                for &c in &cs {
                    if !close_enough(breadth(&a, c, cr) + a[c].extent(cr) / 2.0, vmid) {
                        offcross += 1;
                    }
                }
            }
        }
    }

    assert_eq!(
        uncentred, 0,
        "{uncentred} parents are not centred over their children"
    );
    assert_eq!(
        unordered, 0,
        "{unordered} pairs of siblings are out of order"
    );
    assert_eq!(
        offcross, 0,
        "{offcross} children are not centred under their parent"
    );
}

/// A copy of the subtree of `t` with every family reversed.
fn mirrored(a: &Arena3, t: NodeId, out: &mut Arena3) -> NodeId {
    let n = &a[t];
    let id = out.add_node(n.idx, n.w, n.h, n.d, n.margin);
    let kids: Vec<_> = a
        .children(t)
        .iter()
        .rev()
        .map(|&c| mirrored(a, c, out))
        .collect();
    out.set_children(id, &kids);
    id
}

/// Without margins, which only sit on the far faces, reversing every family reflects the
/// drawing along `x` and along `z`.
#[test]
fn symmetry() {
    let mut asymmetric = 0;
    let mut trees = 0;

    for regime in [Regime::Coarse, Regime::Uniform, Regime::Tiny] {
        for rule in RULES {
            for k in 0..80u64 {
                let (mut a, root) = tree(700 + k, 4, 4, regime);
                for v in a.preorder(root) {
                    a[v].margin = 0.0;
                }
                let mut m = Arena3::new();
                let mroot = mirrored(&a, root, &mut m);

                layout3(
                    &mut a,
                    &LayoutInput3 {
                        rule,
                        ..LayoutInput3::new(root)
                    },
                );
                layout3(
                    &mut m,
                    &LayoutInput3 {
                        rule,
                        ..LayoutInput3::new(mroot)
                    },
                );

                let extent =
                    |a: &Arena3,
                     r: NodeId,
                     q: usize,
                     e: fn(&non_layered_tidy_trees::space::Node3) -> f64| {
                        a.preorder(r)
                            .iter()
                            .map(|&v| lo(a, v)[q] + e(&a[v]))
                            .fold(0.0, f64::max)
                    };
                let (wx, wz) = (extent(&a, root, 0, |n| n.w), extent(&a, root, 2, |n| n.d));

                // the mirror of a node is found by its label
                let mut by_idx = std::collections::HashMap::new();
                for v in m.preorder(mroot) {
                    by_idx.insert(m[v].idx, v);
                }

                trees += 1;
                let bad = a.preorder(root).iter().any(|&v| {
                    let w = by_idx[&a[v].idx];
                    let (p, q) = (lo(&a, v), lo(&m, w));
                    !close_enough(q[0], wx - p[0] - a[v].w)
                        || !close_enough(q[2], wz - p[2] - a[v].d)
                        || p[1] != q[1]
                });
                if bad {
                    asymmetric += 1;
                }
            }
        }
    }

    assert_eq!(
        asymmetric, 0,
        "{asymmetric} of {trees} trees are not mirrored by reversing their families"
    );
}

/// Copies the subtree of `t` from `a` into `out`.
fn copy(a: &Arena3, t: NodeId, out: &mut Arena3, label: &mut usize) -> NodeId {
    let n = &a[t];
    *label += 1;
    let id = out.add_node(*label, n.w, n.h, n.d, n.margin);
    let kids: Vec<_> = a
        .children(t)
        .iter()
        .map(|&c| copy(a, c, out, label))
        .collect();
    out.set_children(id, &kids);
    id
}

/// Two copies of one subtree, on either side of a third, come out congruent.
#[test]
fn congruence() {
    let mut differ = 0;

    for rule in RULES {
        for k in 0..100u64 {
            let (s, sroot) = tree(900 + k, 4, 3, Regime::Coarse);
            let (o, oroot) = tree(1900 + k, 4, 3, Regime::Coarse);

            let mut a = Arena3::new();
            let mut label = 0;
            let root = a.add_node(0, 10.0, 10.0, 10.0, 0.0);
            let left = copy(&s, sroot, &mut a, &mut label);
            let mid = copy(&o, oroot, &mut a, &mut label);
            let right = copy(&s, sroot, &mut a, &mut label);
            a.set_children(root, &[left, mid, right]);

            layout3(
                &mut a,
                &LayoutInput3 {
                    rule,
                    ..LayoutInput3::new(root)
                },
            );

            let (l0, r0) = (lo(&a, left), lo(&a, right));
            for (p, q) in a.preorder(left).into_iter().zip(a.preorder(right)) {
                let (lp, rq) = (lo(&a, p), lo(&a, q));
                if (0..3).any(|i| !close_enough(lp[i] - l0[i], rq[i] - r0[i])) {
                    differ += 1;
                }
            }
        }
    }

    assert_eq!(
        differ, 0,
        "{differ} nodes of identical subtrees are drawn differently"
    );
}

/// Contour pairs compared over a layout, per node.
fn pairs_per_node(a: &mut Arena3, root: NodeId, rule: AxisRule) -> f64 {
    let mut pairs = 0usize;
    let mut count = |_: &Arena3, _: Item, _: Item, _: Axis, _: f64| pairs += 1;
    let mut cb = Callbacks3 {
        walk: None,
        contour_pairs: Some(&mut count),
    };
    layout3_with(
        a,
        &LayoutInput3 {
            rule,
            ..LayoutInput3::new(root)
        },
        &mut cb,
    );
    pairs as f64 / a.len() as f64
}

/// A spine whose every node has a tall thin leaf before the next spine node.
fn spine(n: usize, wide_last: bool) -> (Arena3, NodeId) {
    let mut a = Arena3::new();
    let root = a.add_node(1, 4.0, 1.0, 4.0, 0.0);
    let mut v = root;
    for i in 0..n {
        let leaf = a.add_node(2 * i + 2, 1.0, 40.0, 1.0, 0.0);
        let next = a.add_node(2 * i + 3, 4.0, 1.0, 4.0, 0.0);
        if wide_last {
            a.set_children(v, &[leaf, next]);
        } else {
            a.set_children(v, &[next, leaf]);
        }
        v = next;
    }
    (a, root)
}

/// Builds a tree of about `n` nodes.
type Shape = dyn Fn(usize) -> (Arena3, NodeId);

/// The tree `bench_tree` builds, lifted.
fn bench(n: usize) -> (Arena3, NodeId) {
    let mut flat = Arena::new();
    let root = treegen::bench_tree(&mut flat, n, 5, 42);
    let mut rng = Rng::new(7);
    Arena3::lift(&flat, root, |_| (10 + rng.rand(40)) as f64)
}

#[test]
fn linearity() {
    // the spines are as deep as they are long, and the walks recurse once per level
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(linearity_on_a_large_stack)
        .expect("spawning the linearity thread")
        .join()
        .expect("measuring the contour pairs");
}

fn linearity_on_a_large_stack() {
    let mut worst = Vec::new();

    for rule in RULES {
        let mut shapes: Vec<(&str, Box<Shape>)> = vec![
            ("bench tree", Box::new(bench)),
            ("spine, leaves first", Box::new(|n| spine(n / 2, true))),
            ("spine, leaves last", Box::new(|n| spine(n / 2, false))),
        ];
        shapes.push((
            "binary",
            Box::new(|n| {
                let mut a = Arena3::new();
                let root = a.add_node(1, 3.0, 2.0, 5.0, 0.0);
                let mut frontier = vec![root];
                let mut made = 1;
                while made < n {
                    let mut next = Vec::new();
                    for &v in &frontier {
                        let l = a.add_node(
                            made + 1,
                            3.0 + (made % 3) as f64,
                            2.0 + (made % 5) as f64,
                            5.0,
                            0.0,
                        );
                        let r = a.add_node(made + 2, 4.0, 1.0 + (made % 7) as f64, 3.0, 0.0);
                        a.set_children(v, &[l, r]);
                        next.extend([l, r]);
                        made += 2;
                    }
                    frontier = next;
                }
                (a, root)
            }),
        ));

        for (name, make) in &shapes {
            let ratios: Vec<f64> = [2_000, 8_000, 32_000]
                .iter()
                .map(|&n| {
                    let (mut a, root) = make(n);
                    pairs_per_node(&mut a, root, rule)
                })
                .collect();
            worst.push((format!("{rule:?} {name}"), ratios));
        }
    }

    eprintln!("contour pairs per node at n = 2k, 8k, 32k:");
    for (name, r) in &worst {
        eprintln!("  {name:<32} {:.2} {:.2} {:.2}", r[0], r[1], r[2]);
    }

    let over: Vec<_> = worst
        .iter()
        .filter(|(_, r)| r.iter().any(|&x| x > 4.0))
        .collect();
    assert!(
        over.is_empty(),
        "more than 4 contour pairs per node: {over:?}"
    );

    // and not growing with n either
    let growing: Vec<_> = worst
        .iter()
        .filter(|(_, r)| r[2] > r[0] * 1.25 + 0.05)
        .collect();
    assert!(
        growing.is_empty(),
        "contour pairs per node grow with n: {growing:?}"
    );
}

#[test]
fn a_deep_chain_lays_out() {
    let depth = 10_000;
    let worker = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let mut a = Arena3::new();
            let root = a.add_node(1, 10.0, 10.0, 10.0, 0.0);
            let mut last = root;
            for i in 1..depth {
                let n = a.add_node(i + 1, 10.0, 10.0, 10.0, 0.0);
                a.push_child(last, n);
                last = n;
            }
            layout3(
                &mut a,
                &LayoutInput3 {
                    rule: AxisRule::Alternate,
                    ..LayoutInput3::new(root)
                },
            );
            (a[last].y, a[last].x, a[last].z)
        })
        .expect("spawning the deep-walk thread");

    let (y, x, z) = worker.join().expect("laying out a deep chain");
    assert_eq!((y, x, z), (10.0 * (depth - 1) as f64, 0.0, 0.0));
}

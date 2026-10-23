//! The headline property in three dimensions: no two boxes overlap, and the margin holds
//! along the axis that separates them.
//!
//! The corpus is the one of `tests/overlap.rs`, lifted into 3D with a `d` per box drawn
//! from a stream of its own — so that `treegen` keeps drawing exactly what the C does —
//! and laid out under every axis rule, plus a random axis per node. Every pair of nodes of
//! every tree is checked.
//!
//! ```sh
//! NLTT_TRIALS=10 cargo test --test space_overlap     # a quicker sweep
//! ```

use non_layered_tidy_trees::space::{layout3, Arena3, Axis, AxisRule, LayoutInput3};
use non_layered_tidy_trees::treegen::{self, Regime, Rng, REGIMES};
use non_layered_tidy_trees::NodeId;

const E: f64 = 1e-9;

fn trials() -> usize {
    std::env::var("NLTT_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
}

/// A `d` in the same spirit as the regime's `w`.
fn depth_of(regime: Regime, rng: &mut Rng) -> f64 {
    match regime {
        Regime::Coarse => (10 * (1 + rng.rand(3))) as f64,
        Regime::Uniform => 10.0,
        Regime::Varied => (5 + rng.rand(40)) as f64,
        Regime::Tiny => (1 + rng.rand(4)) as f64,
    }
}

/// The rules swept: the three of `AxisRule`, then a coin flip per node.
const RULES: usize = 4;

fn tree(
    seed: u64,
    maxdepth: usize,
    maxkids: usize,
    regime: Regime,
    rule: usize,
) -> (Arena3, NodeId) {
    let (arena, root) = treegen::build(seed, maxdepth, maxkids, regime);
    let mut rng = Rng::new(seed ^ 0x5DEE_CE66);
    let (mut a3, root3) = Arena3::lift(&arena, root, |_| depth_of(regime, &mut rng));

    if rule == 3 {
        for v in a3.preorder(root3) {
            a3[v].axis = Some(if rng.rand(2) == 0 { Axis::X } else { Axis::Z });
        }
    }

    (a3, root3)
}

fn rule_of(rule: usize) -> AxisRule {
    [
        AxisRule::AllX,
        AxisRule::AllZ,
        AxisRule::Alternate,
        AxisRule::AllX,
    ][rule]
}

/// The corner of a box and its far corner, whatever `centered` says.
fn span(a: &Arena3, t: NodeId) -> ([f64; 3], [f64; 3]) {
    let n = &a[t];
    let (w, h, d) = (n.w, n.h, n.d);
    let lo = if n.centered {
        [n.x - w / 2.0, n.y - h / 2.0, n.z - d / 2.0]
    } else {
        [n.x, n.y, n.z]
    };
    (lo, [lo[0] + w, lo[1] + h, lo[2] + d])
}

fn ancestors(a: &Arena3, mut t: NodeId) -> Vec<NodeId> {
    let mut out = vec![t];
    while let Some(p) = a[t].parent {
        out.push(p);
        t = p;
    }
    out
}

/// The overlapping pairs and the pairs closer than the margin along their lowest common
/// ancestor's family axis, among the nodes whose depth ranges overlap.
fn violations(a: &Arena3, root: NodeId) -> (usize, usize, String) {
    let nodes = a.preorder(root);
    let anc: Vec<Vec<NodeId>> = nodes.iter().map(|&v| ancestors(a, v)).collect();
    let (mut overlaps, mut margins) = (0, 0);
    let mut first = String::new();

    for i in 0..nodes.len() {
        let (p, (plo, phi)) = (nodes[i], span(a, nodes[i]));
        for j in i + 1..nodes.len() {
            let (q, (qlo, qhi)) = (nodes[j], span(a, nodes[j]));

            if !(phi[1] - E > qlo[1] && qhi[1] - E > plo[1]) {
                continue; // apart in depth
            }

            let apart = |k: usize| !(phi[k] - E > qlo[k] && qhi[k] - E > plo[k]);
            if !apart(0) && !apart(2) {
                overlaps += 1;
                if first.is_empty() {
                    first = format!(
                        "#{} {plo:?}..{phi:?} and #{} {qlo:?}..{qhi:?}",
                        a[p].idx, a[q].idx
                    );
                }
            }

            // Preorder puts the earlier branch first: p is before q along a(u).
            if anc[j].contains(&p) {
                continue;
            }
            let u = *anc[i].iter().find(|v| anc[j].contains(v)).unwrap();
            let k = if a[u].family() == Axis::X { 0 } else { 2 };
            if phi[k] + a[p].margin > qlo[k] + E {
                margins += 1;
            }
        }
    }

    (overlaps, margins, first)
}

#[test]
fn no_pair_of_boxes_overlaps() {
    let trials = trials();
    let (mut overlaps, mut margins, mut trees) = (0, 0, 0);
    let mut reported = String::new();

    for regime in REGIMES {
        for maxdepth in 1..=6 {
            for maxkids in 2..=5 {
                for cent in [false, true] {
                    for rule in 0..RULES {
                        for k in 0..trials {
                            let seed = 1000003 * k as u64
                                + 7 * (regime.index() * 100 + maxdepth * 10 + maxkids) as u64;
                            let (mut a, root) = tree(seed, maxdepth, maxkids, regime, rule);
                            let input = LayoutInput3 {
                                centered: cent,
                                rule: rule_of(rule),
                                ..LayoutInput3::new(root)
                            };
                            layout3(&mut a, &input);

                            let (o, m, first) = violations(&a, root);
                            trees += 1;
                            if (o > 0 || m > 0) && reported.len() < 2000 {
                                reported.push_str(&format!(
                                    "\n  regime={} depth={maxdepth} kids={maxkids} cent={cent} \
                                     rule={rule} seed={seed}: {o} overlaps, {m} margins {first}",
                                    regime.index()
                                ));
                            }
                            overlaps += o;
                            margins += m;
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        overlaps + margins,
        0,
        "{overlaps} overlapping pairs and {margins} margin violations over {trees} trees{reported}"
    );
}

/// Boxes with no extent along one axis or more: a zero `d` collapses the cross contour of
/// a family to a plane, a zero `h` makes depth ranges empty.
#[test]
fn degenerate_boxes() {
    let (mut overlaps, mut margins) = (0, 0);

    for k in 0..400u64 {
        let (arena, root) = treegen::build(77 + k, 5, 4, Regime::Tiny);
        let mut rng = Rng::new(k);
        let (mut a, root) = Arena3::lift(&arena, root, |_| rng.rand(3) as f64);
        for v in a.preorder(root) {
            let n = &mut a[v];
            match rng.rand(6) {
                0 => n.w = 0.0,
                1 => n.h = 0.0,
                _ => {}
            }
        }

        for rule in [AxisRule::AllX, AxisRule::AllZ, AxisRule::Alternate] {
            layout3(
                &mut a,
                &LayoutInput3 {
                    rule,
                    ..LayoutInput3::new(root)
                },
            );
            let (o, m, _) = violations(&a, root);
            overlaps += o;
            margins += m;
            for v in a.preorder(root) {
                let n = &a[v];
                assert!(
                    n.x.is_finite() && n.y.is_finite() && n.z.is_finite(),
                    "#{} is not placed",
                    n.idx
                );
            }
        }
    }

    assert_eq!(
        overlaps + margins,
        0,
        "{overlaps} overlapping pairs and {margins} margin violations"
    );
}

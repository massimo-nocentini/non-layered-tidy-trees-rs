//! The 3D layout over real data: trees made from the Bitcoin Payment Graph.
//!
//! The fixtures in `tests/data` are built by `viz/pg_cases.py` and `viz/pg_tenk.py` from
//! the reference fixtures of utxo2webgraph-rs; `tests/data/README.md` says how. They are
//! the cases of the page in `viz/`, and this pins down what that page relies on:
//!
//! * **no overlap** — no two boxes of positive volume overlap, under every rule
//! * **sweeps** — the flat engine gives the same coordinates bit for bit
//! * **2D** — with every family along `x`, `x` and `y` are those of `layout`
//! * **time** — a box starts exactly where its parent's ends, which is what makes the
//!   depth axis block height when box heights are lifetimes
//! * **a square** — the grouping tree of the first 10 000 transactions packs their 9 419
//!   roots into a square under `Alternate` and `Compact`, where `AllX` lays them in a row

use non_layered_tidy_trees::space::flat::Engine3;
use non_layered_tidy_trees::space::{layout3, Arena3, Axis, AxisRule, LayoutInput3};
use non_layered_tidy_trees::{layout, Arena, LayoutInput, NodeId};

const TREES: [&str; 3] = ["pg_tenk.tree", "pg_funnel.tree", "pg_first.tree"];
const RULES: [AxisRule; 3] = [AxisRule::Alternate, AxisRule::Compact, AxisRule::AllX];

/// `w h d margin parent axis` per line, the root first; `#` starts a comment.
fn rows(name: &str) -> Vec<Vec<f64>> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {path}: {e}"))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(|x| x.parse().unwrap()).collect())
        .collect()
}

/// The tree in a 3D arena, with the axes it asks for unless the rule is `AllX`.
fn tree(rows: &[Vec<f64>], rule: AxisRule) -> (Arena3, Vec<NodeId>) {
    let mut a = Arena3::with_capacity(rows.len());
    let ids: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| a.add_node(i + 1, r[0], r[1], r[2], r[3]))
        .collect();
    for (i, r) in rows.iter().enumerate() {
        if r[4] >= 0.0 {
            a.push_child(ids[r[4] as usize], ids[i]);
        }
        if rule != AxisRule::AllX {
            a[ids[i]].axis = match r[5] as i64 {
                0 => Some(Axis::X),
                1 => Some(Axis::Z),
                _ => None,
            };
        }
    }
    (a, ids)
}

fn laid_out(name: &str, rule: AxisRule) -> (Arena3, Vec<NodeId>) {
    let (mut a, ids) = tree(&rows(name), rule);
    layout3(
        &mut a,
        &LayoutInput3 {
            rule,
            ..LayoutInput3::new(ids[0])
        },
    );
    (a, ids)
}

/// Overlapping pairs among the boxes of positive volume, by a sweep down the depth axis.
fn overlaps(a: &Arena3, ids: &[NodeId]) -> usize {
    const E: f64 = 1e-9;
    let apart = |lo1: f64, e1: f64, lo2: f64, e2: f64| !(lo1 + e1 - E > lo2 && lo2 + e2 - E > lo1);
    let mut boxes: Vec<NodeId> = ids
        .iter()
        .copied()
        .filter(|&v| a[v].w > 0.0 && a[v].h > 0.0 && a[v].d > 0.0)
        .collect();
    boxes.sort_by(|&p, &q| a[p].y.total_cmp(&a[q].y));

    let mut bad = 0;
    for (i, &p) in boxes.iter().enumerate() {
        let p = &a[p];
        for &q in &boxes[i + 1..] {
            let q = &a[q];
            if q.y >= p.y + p.h - E {
                break;
            }
            if !apart(p.x, p.w, q.x, q.w) && !apart(p.z, p.d, q.z, q.d) {
                bad += 1;
            }
        }
    }
    bad
}

fn footprint(a: &Arena3, ids: &[NodeId]) -> (f64, f64) {
    ids.iter().fold((0.0f64, 0.0f64), |(x, z), &v| {
        (x.max(a[v].x + a[v].w), z.max(a[v].z + a[v].d))
    })
}

#[test]
fn no_pair_of_boxes_overlaps() {
    for name in TREES {
        for rule in RULES {
            let (a, ids) = laid_out(name, rule);
            assert_eq!(overlaps(&a, &ids), 0, "{name} under {rule:?}");
        }
    }
}

#[test]
fn the_sweeps_agree_with_the_recursion() {
    let mut engine = Engine3::new();
    for name in TREES {
        for rule in RULES {
            let (a, ids) = laid_out(name, rule);
            let (mut b, _) = tree(&rows(name), rule);
            engine.layout(
                &mut b,
                &LayoutInput3 {
                    rule,
                    ..LayoutInput3::new(ids[0])
                },
            );
            for &v in &ids {
                assert_eq!(
                    [a[v].x, a[v].y, a[v].z].map(f64::to_bits),
                    [b[v].x, b[v].y, b[v].z].map(f64::to_bits),
                    "{name} under {rule:?}: box {} differs",
                    a[v].idx
                );
            }
        }
    }
}

#[test]
fn along_x_it_is_the_2d_drawing() {
    for name in TREES {
        let rows = rows(name);
        let mut flat = Arena::with_capacity(rows.len());
        let ids: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| flat.add_node(i + 1, r[0], r[1], r[3], false))
            .collect();
        for (i, r) in rows.iter().enumerate() {
            if r[4] >= 0.0 {
                flat.push_child(ids[r[4] as usize], ids[i]);
            }
        }
        layout(&mut flat, &LayoutInput::new(ids[0]));

        let (a, ids3) = laid_out(name, AxisRule::AllX);
        for (&p, &q) in ids.iter().zip(&ids3) {
            assert_eq!(
                (flat[p].x.to_bits(), flat[p].y.to_bits()),
                (a[q].x.to_bits(), a[q].y.to_bits()),
                "{name}: box {} differs from `layout`",
                a[q].idx
            );
        }
    }
}

#[test]
fn a_box_starts_where_its_parent_ends() {
    for name in TREES {
        let (a, ids) = laid_out(name, AxisRule::Alternate);
        for &v in &ids {
            if let Some(p) = a[v].parent {
                assert_eq!(a[v].y, a[p].y + a[p].h, "{name}: box {}", a[v].idx);
            }
        }
    }
}

#[test]
fn the_first_ten_thousand_transactions_pack_into_a_square() {
    for rule in [AxisRule::Alternate, AxisRule::Compact] {
        let (a, ids) = laid_out("pg_tenk.tree", rule);
        let (x, z) = footprint(&a, &ids);
        assert!(
            x.max(z) < 1400.0 && x.max(z) < 1.1 * x.min(z),
            "{rule:?}: the 9 419 roots spread over {x:.1} x {z:.1}"
        );
    }

    let (a, ids) = laid_out("pg_tenk.tree", AxisRule::AllX);
    let (x, _) = footprint(&a, &ids);
    assert!(x > 70_000.0, "along x the roots are one row, {x:.1} long");
}

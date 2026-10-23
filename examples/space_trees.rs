//! Lays tree files out with `layout3` under every rule, for the page in `viz/`.
//!
//! ```text
//! cargo run --release --example space_trees -- tests/data/pg_tenk.tree viz/out/random.tree
//! ```
//!
//! A tree file has one box per line, `w h d margin parent axis`, the root first with parent
//! `-1` and every parent before its children; `axis` is `0` for `x`, `1` for `z` and `-1` to
//! leave it to the rule. Lines starting with `#` are comments.
//!
//! Each tree is laid out under `Alternate`, `Compact` and `AllX`, the last with every axis
//! cleared, and every layout is checked twice before it is printed: the flat engine has to
//! give the same coordinates bit for bit, and no two boxes of positive volume may overlap.
//! Standard output gets one JSON object per tree, in the order of the arguments:
//! `{"y": [..], "alternate": [x, z, family, ..], "compact": [..], "allx": [..]}`, with `y`
//! the depth every rule shares and `family` `0` for `x`, `1` for `z`, `-1` for a leaf.

use non_layered_tidy_trees::space::flat::Engine3;
use non_layered_tidy_trees::space::{layout3, Arena3, Axis, AxisRule, LayoutInput3};
use non_layered_tidy_trees::NodeId;

/// Reads a tree file into an arena; the root is the first node.
fn read(path: &str) -> (Arena3, Vec<NodeId>, Vec<Option<Axis>>) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let rows: Vec<Vec<f64>> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            l.split_whitespace()
                .map(|x| x.parse().expect("a number"))
                .collect()
        })
        .collect();

    let mut arena = Arena3::with_capacity(rows.len());
    let ids: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| arena.add_node(i + 1, r[0], r[1], r[2], r[3]))
        .collect();
    let mut axes = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        if r[4] >= 0.0 {
            arena.push_child(ids[r[4] as usize], ids[i]);
        }
        axes.push(match r.get(5).map(|&a| a as i64) {
            Some(0) => Some(Axis::X),
            Some(1) => Some(Axis::Z),
            _ => None,
        });
    }
    (arena, ids, axes)
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

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: space_trees <tree>...");

    let mut out = Vec::new();
    for path in &paths {
        let (mut a, ids, axes) = read(path);
        let mut fields = Vec::new();

        for (name, rule) in [
            ("alternate", AxisRule::Alternate),
            ("compact", AxisRule::Compact),
            ("allx", AxisRule::AllX),
        ] {
            for (&v, &ax) in ids.iter().zip(&axes) {
                a[v].axis = if rule == AxisRule::AllX { None } else { ax };
            }
            let input = LayoutInput3 {
                rule,
                ..LayoutInput3::new(ids[0])
            };
            let mut b = a.clone();
            layout3(&mut a, &input);
            Engine3::new().layout(&mut b, &input);

            let same = ids.iter().all(|&v| {
                [a[v].x, a[v].y, a[v].z].map(f64::to_bits)
                    == [b[v].x, b[v].y, b[v].z].map(f64::to_bits)
            });
            let bad = overlaps(&a, &ids);
            let (mut mx, mut mz) = (0.0f64, 0.0f64);
            for &v in &ids {
                mx = mx.max(a[v].x + a[v].w);
                mz = mz.max(a[v].z + a[v].d);
            }
            eprintln!(
                "{path}: {} nodes, {name:<9} footprint {mx:.1} x {mz:.1}, {bad} overlapping pairs, \
                 flat engine {}",
                ids.len(),
                if same { "identical" } else { "DIFFERENT" }
            );
            assert!(same && bad == 0, "{path} under {name} fails its checks");

            if fields.is_empty() {
                let y: Vec<String> = ids.iter().map(|&v| a[v].y.to_string()).collect();
                fields.push(format!("\"y\":[{}]", y.join(",")));
            }
            let round = |v: f64| (v * 100.0).round() / 100.0;
            let xz: Vec<String> = ids
                .iter()
                .map(|&v| {
                    let n = &a[v];
                    let f = match (n.children().is_empty(), n.family()) {
                        (true, _) => -1,
                        (false, Axis::X) => 0,
                        (false, _) => 1,
                    };
                    format!("{},{},{f}", round(n.x), round(n.z))
                })
                .collect();
            fields.push(format!("\"{name}\":[{}]", xz.join(",")));
        }

        out.push(format!("{{{}}}", fields.join(",")));
    }

    println!("[{}]", out.join(","));
}

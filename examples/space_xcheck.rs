//! The Rust half of `prototype/xcheck.py`: lays out the trees of a file with `layout3`.
//!
//! The file holds trees one after the other, each a `tree <n>` line followed by `n` lines of
//! `w h d margin parent axis`, the root first with parent `-1` and the axis `0` for `x`, `1`
//! for `z`. One line comes out per tree, with `x y z` for every node in the same order.

use non_layered_tidy_trees::space::{layout3, Arena3, Axis, LayoutInput3};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: space_xcheck <trees>");
    let text = std::fs::read_to_string(&path).expect("reading the trees");
    let mut lines = text.lines();

    while let Some(head) = lines.next() {
        let n: usize = head
            .strip_prefix("tree ")
            .and_then(|n| n.parse().ok())
            .expect("a `tree <n>` line");

        let mut arena = Arena3::with_capacity(n);
        let rows: Vec<Vec<f64>> = (0..n)
            .map(|_| {
                let line = lines.next().expect("a node line");
                line.split(' ')
                    .map(|x| x.parse().expect("a number"))
                    .collect()
            })
            .collect();
        let ids: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| arena.add_node(i + 1, r[0], r[1], r[2], r[3]))
            .collect();

        for (i, r) in rows.iter().enumerate() {
            arena[ids[i]].axis = Some(if r[5] == 0.0 { Axis::X } else { Axis::Z });
            if r[4] >= 0.0 {
                arena.push_child(ids[r[4] as usize], ids[i]);
            }
        }

        layout3(&mut arena, &LayoutInput3::new(ids[0]));

        let coords: Vec<String> = ids
            .iter()
            .map(|&v| format!("{} {} {}", arena[v].x, arena[v].y, arena[v].z))
            .collect();
        println!("{}", coords.join(" "));
    }
}

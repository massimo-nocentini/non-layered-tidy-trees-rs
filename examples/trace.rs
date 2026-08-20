//! What `NLTT_TRACE` prints, over one small tree laid out by every entry point.
//!
//! ```text
//! NLTT_TRACE=1 cargo run --example trace
//! ```
//!
//! Without the variable the run is silent, which is the point: the instrumentation is one
//! cached load per phase and nothing else.

use non_layered_tidy_trees::{flat, layout, layout_api, Arena, LayoutInput};

fn main() {
    let mut arena = Arena::new();

    let root = arena.add_node(1, 10.0, 10.0, 0.0, false);
    let a = arena.add_node(2, 10.0, 10.0, 0.0, false);
    let b = arena.add_node(3, 10.0, 30.0, 0.0, false);
    let c = arena.add_node(4, 10.0, 10.0, 0.0, false);

    arena.set_children(root, &[a, b]);
    arena.set_children(a, &[c]);

    let input = LayoutInput::new(root);

    // the recursive walks, then the sweeps, then the sweeps with their own stopwatch
    layout(&mut arena, &input);
    flat::layout_flat(&mut arena, &input);
    flat::Engine::new().profile(&mut arena, &input);

    // the same tree as parallel arrays: four nodes, the root first, each with a gap node
    let mut wh = vec![
        10.0, 10.0, 10.0, 10.0, // widths
        10.0, 10.0, 30.0, 10.0, // heights
        0.0, 0.0, 0.0, 0.0, // margins
    ];
    let whg = vec![0.0, 0.0, 0.0, 0.0, 5.0, 5.0, 5.0, 5.0];
    let children = vec![2, 1, 0, 0, 2, 3, 4];

    layout_api(4, &mut wh, &whg, &children, 1, true, false, 0.0, 0.0);
    flat::layout_api_flat(4, &mut wh, &whg, &children, 1, true, false, 0.0, 0.0);
}

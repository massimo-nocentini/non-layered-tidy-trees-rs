//! The 3D layout over a struct-of-arrays mirror, laid out by sweeps instead of recursion.
//!
//! [`layout3_flat`] computes exactly what [`super::layout3`] computes, bit for bit, the way
//! [`crate::flat`] does for the 2D layout: the tree is copied into a mirror in breadth-first
//! order, so the children of a node are a contiguous range and a parent always comes
//! before its children; the walks become forward or backward sweeps over that mirror; and
//! nothing recurses, so the depth of a tree is bounded by memory and not by the stack.
//!
//! A contour item is an index: entry `v` for its own box, `n + 2v` and `n + 2v + 1` for its
//! left and right hull.
//!
//! [`layout_api3_flat`] fills the mirror straight from the arrays of [`super::layout_api3`].
//! Each gap node is an entry of its own here, where [`crate::flat`] folds it into a band of
//! the node below it: a fold would have to carry a second set of hulls, for one node in two.
//!
//! The sweeps visit nodes in a different order than the recursion, which is why this path
//! takes no callbacks, like [`crate::flat`].

use super::{deltas, heights, offsets, ApiInput3, Arena3, Axis, Frame, LayoutInput3};
use crate::{update_chain, ChainLink, NodeId};

/// The absence of an item.
const NONE: usize = usize::MAX;

const LEFT: usize = 0;
const RIGHT: usize = 1;

/// Lays out the tree with the sweeps, writing `x`, `y` and `z` back into the arena.
///
/// Equivalent to [`super::layout3`] in every observable way, except that it takes no
/// callbacks.
///
/// # Panics
///
/// As [`super::layout3`] does.
pub fn layout3_flat(arena: &mut Arena3, input: &LayoutInput3) {
    Engine3::new().layout(arena, input);
}

/// [`super::layout_api3`], without ever building an arena.
///
/// The arguments are exactly [`super::layout_api3`]'s, and so is the result, bit for bit.
///
/// # Panics
///
/// As [`super::layout_api3`] does.
pub fn layout_api3_flat(
    n: usize,
    whd: &mut [f64],
    gaps: &[f64],
    children: &[usize],
    rooti: usize,
    input: &ApiInput3,
) {
    Engine3::new().layout_api(n, whd, gaps, children, rooti, input);
}

/// A reusable mirror, for laying out repeatedly; the counterpart of [`crate::flat::Engine`].
///
/// ```
/// use non_layered_tidy_trees::space::{flat::Engine3, Arena3, LayoutInput3};
///
/// let mut arena = Arena3::new();
/// let root = arena.add_node(1, 10.0, 10.0, 10.0, 0.0);
/// let leaf = arena.add_node(2, 10.0, 10.0, 10.0, 0.0);
/// arena.set_children(root, &[leaf]);
///
/// let mut engine = Engine3::new();
/// let input = LayoutInput3::new(root);
///
/// engine.layout(&mut arena, &input);
/// engine.layout(&mut arena, &input); // no allocation this time
///
/// assert_eq!((arena[leaf].x, arena[leaf].y, arena[leaf].z), (0.0, 10.0, 0.0));
/// ```
#[derive(Debug, Default)]
pub struct Engine3 {
    m: Mirror,
}

impl Engine3 {
    /// An engine that has yet to allocate anything.
    pub fn new() -> Engine3 {
        Engine3::default()
    }

    /// Lays out the tree, reusing whatever this engine already allocated.
    pub fn layout(&mut self, arena: &mut Arena3, input: &LayoutInput3) {
        let frame = Frame::new(input.depth);
        let on = crate::trace::enabled();
        let mut sum = std::time::Duration::ZERO;

        trace!(
            "layout3_flat root=#{} arena={} depth={:?} centered={} rule={:?} origin=({}, {}, {})",
            arena[input.root].idx,
            arena.len(),
            input.depth,
            input.centered,
            input.rule,
            input.x,
            input.y,
            input.z
        );

        let t = crate::trace::start();
        self.m.rebuild(arena, input.root, frame);
        sum += phase!(t, "build", "nodes={}", self.m.n);

        let t = crate::trace::start();
        self.m.resolve(input, frame);
        sum += phase!(t, "setup");

        sum += self
            .m
            .sweeps(frame, input.centered, [input.x, input.y, input.z]);

        let t = crate::trace::start();
        self.m.write_back(arena, input.centered, frame);
        sum += phase!(t, "write", "{} nodes", self.m.n);

        total!(on, sum);
    }

    /// [`layout_api3_flat`], reusing whatever this engine already allocated.
    pub fn layout_api(
        &mut self,
        n: usize,
        whd: &mut [f64],
        gaps: &[f64],
        children: &[usize],
        rooti: usize,
        input: &ApiInput3,
    ) {
        let frame = Frame::new(input.depth);
        let on = crate::trace::enabled();
        let mut sum = std::time::Duration::ZERO;

        trace!(
            "layout_api3_flat  n={n} rooti={rooti} depth={:?} centered={} rule={:?} origin=({}, {}, {})",
            input.depth,
            input.centered,
            input.rule,
            input.x,
            input.y,
            input.z
        );

        let t = crate::trace::start();
        self.m
            .rebuild_from_arrays(n, whd, gaps, children, rooti, input, frame);
        sum += phase!(t, "build", "entries={}", self.m.n);

        sum += self
            .m
            .sweeps(frame, input.centered, [input.x, input.y, input.z]);

        let t = crate::trace::start();
        self.m.write_back_arrays(n, whd);
        sum += phase!(t, "write", "{n} coordinate triples");

        total!(on, sum);
    }
}

/// The state of a hull; see `super::Hull`.
#[derive(Debug, Clone, Copy)]
struct Hull {
    bottom: f64,
    prelim: f64,
    modifier: f64,
    thread: usize,
}

const EMPTY_HULL: Hull = Hull {
    bottom: 0.0,
    prelim: 0.0,
    modifier: 0.0,
    thread: NONE,
};

/// The tree, transposed: one array per field, in breadth-first order and in the frame of
/// the layout, the breadth axes indexed 0 and 1.
#[derive(Debug, Default)]
struct Mirror {
    n: usize,

    ext: Vec<[f64; 2]>,
    dext: Vec<f64>,
    margin: Vec<f64>,
    /// What each node asks for, when the mirror comes from an arena.
    axis: Vec<Option<Axis>>,
    fam: Vec<usize>,

    top: Vec<f64>,
    bot: Vec<f64>,

    prelim: Vec<[f64; 2]>,
    modifier: Vec<[f64; 2]>,
    shift: Vec<f64>,
    change: Vec<f64>,
    tl: Vec<[usize; 2]>,
    tr: Vec<[usize; 2]>,
    el: Vec<[usize; 2]>,
    er: Vec<[usize; 2]>,
    msel: Vec<[f64; 2]>,
    mser: Vec<[f64; 2]>,
    maxdepth: Vec<f64>,
    bbox: Vec<[f64; 4]>,
    /// Two per entry, left then right.
    hulls: Vec<Hull>,
    modsum: Vec<[f64; 2]>,
    /// `x`, `y`, `z`.
    coord: Vec<[f64; 3]>,

    parent: Vec<usize>,
    childno: Vec<usize>,
    first_child: Vec<usize>,
    child_count: Vec<usize>,
    level: Vec<usize>,

    /// Where each entry came from, when the mirror was built from an arena.
    id: Vec<NodeId>,
    /// Which node of the flat API each entry is; gap entries carry `n + i` for node `i`.
    src: Vec<usize>,
    /// How many content nodes the mirror reached.
    content: usize,

    chain: Vec<ChainLink>,
}

/// `v` becomes `n` copies of `zero`, keeping whatever it already allocated.
fn reset<T: Copy>(v: &mut Vec<T>, n: usize, zero: T) {
    v.clear();
    if v.capacity() < n {
        v.reserve_exact(n - v.capacity());
    }
    v.resize(n, zero);
}

impl Mirror {
    fn clear_shape(&mut self) {
        self.ext.clear();
        self.dext.clear();
        self.margin.clear();
        self.axis.clear();
        self.fam.clear();
        self.parent.clear();
        self.childno.clear();
        self.first_child.clear();
        self.child_count.clear();
        self.level.clear();
        self.id.clear();
        self.src.clear();
    }

    fn push(&mut self, world: [f64; 3], margin: f64, frame: Frame, parent: usize, childno: usize) {
        self.ext
            .push([world[frame.breadth[0]], world[frame.breadth[1]]]);
        self.dext.push(world[frame.depth]);
        self.margin.push(margin);
        self.parent.push(parent);
        self.childno.push(childno);
        self.first_child.push(0);
        self.child_count.push(0);
        self.level.push(if parent == NONE {
            0
        } else {
            self.level[parent] + 1
        });
    }

    /// Fills the mirror from the arena, breadth-first.
    fn rebuild(&mut self, arena: &Arena3, root: NodeId, frame: Frame) {
        self.clear_shape();

        let add = |m: &mut Mirror, id: NodeId, parent: usize, childno: usize| {
            let node = &arena[id];
            m.push(
                [node.w, node.h, node.d],
                node.margin,
                frame,
                parent,
                childno,
            );
            m.axis.push(node.axis);
            m.id.push(id);
        };

        add(self, root, NONE, 0);

        let mut i = 0;
        while i < self.id.len() {
            self.first_child[i] = self.id.len();
            let children = arena.children(self.id[i]);
            self.child_count[i] = children.len();
            for (k, &c) in children.iter().enumerate() {
                add(self, c, i, k);
            }
            i += 1;
        }

        self.n = self.id.len();
        self.content = self.n;
        self.reset_state();
    }

    /// Fills the mirror from the arrays of the flat API: the gap node of the root first,
    /// then breadth-first, every node right below its own gap node.
    #[allow(clippy::too_many_arguments)]
    fn rebuild_from_arrays(
        &mut self,
        n: usize,
        whd: &[f64],
        gaps: &[f64],
        children: &[usize],
        rooti: usize,
        input: &ApiInput3,
        frame: Frame,
    ) {
        assert!(
            whd.len() >= 4 * n,
            "`whd` holds w, h, d and margin for {n} nodes"
        );
        assert!(
            gaps.len() >= 3 * n,
            "`gaps` holds w, h and d for {n} gap nodes"
        );
        assert!(
            (1..=n).contains(&rooti),
            "`rooti` is a one-based index into {n} nodes"
        );

        let off = offsets(n, children);
        let height = heights(n, children, &off, rooti);

        self.clear_shape();

        let gap = |m: &mut Mirror, s: usize, parent: usize, childno: usize| {
            m.push(
                [gaps[s], gaps[s + n], gaps[s + 2 * n]],
                0.0,
                frame,
                parent,
                childno,
            );
            m.src.push(n + s);
        };

        gap(self, rooti - 1, NONE, 0);

        let mut content = 0;
        let mut e = 0;
        while e < self.src.len() {
            self.first_child[e] = self.src.len();
            let s = self.src[e];

            if s >= n {
                // a gap node: the node it spaces, and nothing else
                let s = s - n;
                self.child_count[e] = 1;
                self.push(
                    [whd[s], whd[s + n], whd[s + 2 * n]],
                    whd[s + 3 * n],
                    frame,
                    e,
                    0,
                );
                self.src.push(s);
                content += 1;
            } else {
                self.child_count[e] = children[s];
                for k in 0..children[s] {
                    gap(self, children[off[s] + k] - 1, e, k);
                }
            }
            e += 1;
        }

        // The family axes go by the heights of the tree without its gap nodes, and a gap
        // node takes the axis of the node below it, as `reify3` sets them.
        for e in 0..self.src.len() {
            let s = self.src[e] % n;
            let fam = if children[s] == 0 {
                0
            } else {
                frame.family(None, input.rule, height[s])
            };
            self.fam.push(fam);
        }

        self.n = self.src.len();
        self.content = content;
        self.reset_state();
    }

    fn reset_state(&mut self) {
        let n = self.n;
        reset(&mut self.top, n, 0.0);
        reset(&mut self.bot, n, 0.0);
        reset(&mut self.prelim, n, [0.0; 2]);
        reset(&mut self.modifier, n, [0.0; 2]);
        reset(&mut self.shift, n, 0.0);
        reset(&mut self.change, n, 0.0);
        reset(&mut self.tl, n, [NONE; 2]);
        reset(&mut self.tr, n, [NONE; 2]);
        reset(&mut self.el, n, [NONE; 2]);
        reset(&mut self.er, n, [NONE; 2]);
        reset(&mut self.msel, n, [0.0; 2]);
        reset(&mut self.mser, n, [0.0; 2]);
        reset(&mut self.maxdepth, n, 0.0);
        reset(&mut self.bbox, n, [0.0; 4]);
        reset(&mut self.hulls, 2 * n, EMPTY_HULL);
        reset(&mut self.modsum, n, [0.0; 2]);
        reset(&mut self.coord, n, [0.0; 3]);
    }

    /// The family axes, by the nodes' own axes and the rule over the subtree heights: a
    /// backward sweep, children before parents.
    fn resolve(&mut self, input: &LayoutInput3, frame: Frame) {
        let mut height = vec![0usize; self.n];
        self.fam.clear();
        self.fam.resize(self.n, 0);

        for v in (0..self.n).rev() {
            if self.child_count[v] == 0 {
                continue;
            }
            let mut h = 0;
            for c in self.children(v) {
                if height[c] + 1 > h {
                    h = height[c] + 1;
                }
            }
            height[v] = h;
            self.fam[v] = frame.family(self.axis[v], input.rule, h);
        }
    }

    fn children(&self, v: usize) -> std::ops::Range<usize> {
        let f = self.first_child[v];
        f..f + self.child_count[v]
    }

    /// The three sweeps and the normalization; what the traced phases added up to.
    fn sweeps(&mut self, frame: Frame, centered: bool, origin: [f64; 3]) -> std::time::Duration {
        let mut sum = std::time::Duration::ZERO;

        let t = crate::trace::start();
        self.setup_sweep();
        sum += phase!(t, "depth");

        let t = crate::trace::start();
        self.first_sweep();
        sum += phase!(t, "first");

        let t = crate::trace::start();
        let mins = self.second_sweep(frame, centered);
        sum += phase!(t, "second", "min={mins:?}");

        let delta = deltas(frame, mins, origin);

        let t = crate::trace::start();
        if delta.iter().any(|&d| d != 0.0) {
            for c in self.coord.iter_mut() {
                c[0] -= delta[0];
                c[1] -= delta[1];
                c[2] -= delta[2];
            }
            sum += phase!(t, "third", "delta={delta:?}");
        } else {
            sum += phase!(t, "third", "already at the origin");
        }

        sum
    }

    /// `setup_walk`'s depth axis: every child starts at its parent's bottom.
    fn setup_sweep(&mut self) {
        self.top[0] = 0.0;
        for v in 0..self.n {
            let b = self.top[v] + self.dext[v];
            self.bot[v] = b;
            for c in self.children(v) {
                self.top[c] = b;
            }
        }
    }

    fn item_bottom(&self, it: usize) -> f64 {
        if it < self.n {
            self.bot[it]
        } else {
            self.hulls[it - self.n].bottom
        }
    }

    fn item_modifier(&self, it: usize, ax: usize) -> f64 {
        if it < self.n {
            self.modifier[it][ax]
        } else {
            self.hulls[it - self.n].modifier
        }
    }

    fn near(&self, sum: f64, it: usize, ax: usize) -> f64 {
        if it < self.n {
            sum + self.prelim[it][ax]
        } else {
            sum + self.hulls[it - self.n].prelim
        }
    }

    fn far(&self, sum: f64, it: usize, ax: usize) -> f64 {
        if it < self.n {
            sum + self.prelim[it][ax] + self.ext[it][ax] + self.margin[it]
        } else {
            sum + self.hulls[it - self.n].prelim
        }
    }

    fn nudge(&mut self, it: usize, ax: usize, diff: f64) {
        if it < self.n {
            self.modifier[it][ax] += diff;
            self.prelim[it][ax] -= diff;
        } else {
            let h = &mut self.hulls[it - self.n];
            h.modifier += diff;
            h.prelim -= diff;
        }
    }

    fn set_thread(&mut self, it: usize, side: usize, ax: usize, to: usize) {
        if it < self.n {
            if side == LEFT {
                self.tl[it][ax] = to;
            } else {
                self.tr[it][ax] = to;
            }
        } else {
            self.hulls[it - self.n].thread = to;
        }
    }

    fn next(&self, it: usize, side: usize, ax: usize) -> usize {
        if it >= self.n {
            return self.hulls[it - self.n].thread;
        }
        let count = self.child_count[it];
        if count == 0 {
            if side == LEFT {
                self.tl[it][ax]
            } else {
                self.tr[it][ax]
            }
        } else if self.fam[it] == ax {
            if side == LEFT {
                self.first_child[it]
            } else {
                self.first_child[it] + count - 1
            }
        } else if count == 1 {
            self.first_child[it]
        } else {
            self.n + 2 * it + side
        }
    }

    fn move_subtree(&mut self, t: usize, i: usize, si: usize, dist: f64, ax: usize) {
        let base = self.first_child[t];
        let ci = base + i;
        self.modifier[ci][ax] += dist;
        self.msel[ci][ax] += dist;
        self.mser[ci][ax] += dist;

        if si + 1 != i {
            let nr = (i - si) as f64;
            let ratio = dist / nr;
            self.shift[base + si + 1] += ratio;
            self.shift[ci] -= ratio;
            self.change[ci] -= dist - ratio;
        }
    }

    fn set_left_thread(&mut self, t: usize, i: usize, cl: usize, modsumcl: f64, ax: usize) {
        let c0 = self.first_child[t];
        let ci = c0 + i;
        let li = self.el[c0][ax];

        let diff = (modsumcl - self.item_modifier(cl, ax)) - self.msel[c0][ax];

        self.set_thread(li, LEFT, ax, cl);
        self.nudge(li, ax, diff);

        self.el[c0][ax] = self.el[ci][ax];
        self.msel[c0][ax] = self.msel[ci][ax];
    }

    fn set_right_thread(&mut self, t: usize, i: usize, sr: usize, modsumsr: f64, ax: usize) {
        let ci = self.first_child[t] + i;
        let cp = ci - 1;
        let ri = self.er[ci][ax];
        let diff = (modsumsr - self.item_modifier(sr, ax)) - self.mser[ci][ax];

        self.set_thread(ri, RIGHT, ax, sr);
        self.nudge(ri, ax, diff);

        self.er[ci][ax] = self.er[cp][ax];
        self.mser[ci][ax] = self.mser[cp][ax];
    }

    fn separate(&mut self, t: usize, i: usize, chain: &[ChainLink], ax: usize) {
        let base = self.first_child[t];
        let mut sr = base + i - 1;
        let mut mssr = self.modifier[sr][ax];
        let mut cl = base + i;
        let mut mscl = self.modifier[cl][ax];

        let mut ih = chain.len() - 1;
        let mut first = true;

        while sr != NONE && cl != NONE {
            let sy = self.item_bottom(sr);
            let cy = self.item_bottom(cl);

            if sy > chain[ih].low {
                ih = ih.checked_sub(1).expect(
                    "the sibling chain is seeded with a value that dominates the subtree depth",
                );
            }

            let dist = self.far(mssr, sr, ax) - self.near(mscl, cl, ax);

            if (first && dist < 0.0) || dist > 0.0 {
                mscl += dist;
                self.move_subtree(t, i, chain[ih].index, dist, ax);
            }

            first = false;

            if sy <= cy {
                sr = self.next(sr, RIGHT, ax);
                if sr != NONE {
                    mssr += self.item_modifier(sr, ax);
                }
            }

            if sy >= cy {
                cl = self.next(cl, LEFT, ax);
                if cl != NONE {
                    mscl += self.item_modifier(cl, ax);
                }
            }
        }

        if sr == NONE && cl != NONE {
            self.set_left_thread(t, i, cl, mscl, ax);
        } else if sr != NONE && cl == NONE {
            self.set_right_thread(t, i, sr, mssr, ax);
        }
    }

    fn set_extreme(&mut self, t: usize, side: usize, ax: usize, it: usize, ms: f64) {
        if side == LEFT {
            self.el[t][ax] = it;
            self.msel[t][ax] = ms;
        } else {
            self.er[t][ax] = it;
            self.mser[t][ax] = ms;
        }
    }

    /// See `super::hull_contour`.
    fn hull_contour(&mut self, t: usize, b: usize, side: usize, j1: usize, d2: f64) {
        let extreme = |x: f64, y: f64| {
            if side == LEFT {
                if y < x {
                    y
                } else {
                    x
                }
            } else if y > x {
                y
            } else {
                x
            }
        };

        let base = self.first_child[t];
        let deep = base + j1;
        let mut it = deep;
        let mut s = self.modifier[deep][b];
        let mut ext = if side == LEFT {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
        let tee = loop {
            let e = if side == LEFT {
                self.near(s, it, b)
            } else {
                self.far(s, it, b)
            };
            ext = extreme(ext, e);
            if self.item_bottom(it) > d2 {
                break Some((it, s));
            }
            let n = self.next(it, side, b);
            if n == NONE {
                break None;
            }
            s += self.item_modifier(n, b);
            it = n;
        };

        for j in 0..self.child_count[t] {
            if j == j1 {
                continue;
            }
            let c = base + j;
            let rel = self.bbox[c][2 * b + side];
            ext = extreme(ext, self.prelim[c][b] + self.modifier[c][b] + rel);
        }

        let hull = match tee {
            Some((tee, st)) => {
                let m = st - self.item_modifier(tee, b);
                let (el, ms) = if side == LEFT {
                    (self.el[deep][b], self.msel[deep][b])
                } else {
                    (self.er[deep][b], self.mser[deep][b])
                };
                self.set_extreme(t, side, b, el, ms);
                Hull {
                    bottom: d2,
                    prelim: ext - m,
                    modifier: m,
                    thread: tee,
                }
            }
            None => {
                self.set_extreme(t, side, b, self.n + 2 * t + side, 0.0);
                Hull {
                    bottom: d2,
                    prelim: ext,
                    modifier: 0.0,
                    thread: NONE,
                }
            }
        };
        self.hulls[2 * t + side] = hull;
    }

    /// `first_walk`: a backward sweep, so every child is done before its parent.
    fn first_sweep(&mut self) {
        let mut chain = std::mem::take(&mut self.chain);

        for t in (0..self.n).rev() {
            let k = self.child_count[t];

            if k == 0 {
                self.el[t] = [t; 2];
                self.er[t] = [t; 2];
                self.msel[t] = [0.0; 2];
                self.mser[t] = [0.0; 2];
                self.bbox[t] = [
                    0.0,
                    self.ext[t][0] + self.margin[t],
                    0.0,
                    self.ext[t][1] + self.margin[t],
                ];
                self.maxdepth[t] = self.bot[t];
                continue;
            }

            let ax = self.fam[t];
            let b = 1 - ax;
            let c0 = self.first_child[t];
            let last = c0 + k - 1;

            chain.clear();
            update_chain(&mut chain, self.item_bottom(self.el[c0][ax]), 0);

            for i in 1..k {
                let min = self.item_bottom(self.er[c0 + i][ax]);
                self.separate(t, i, &chain, ax);
                update_chain(&mut chain, min, i);
            }

            // position_root
            let d = self.ext[last][ax] - self.ext[t][ax];
            self.prelim[t][ax] = (self.prelim[c0][ax]
                + self.modifier[c0][ax]
                + self.prelim[last][ax]
                + self.modifier[last][ax]
                + d)
                / 2.0;

            let (el, msel) = (self.el[c0][ax], self.msel[c0][ax]);
            let (er, mser) = (self.er[last][ax], self.mser[last][ax]);
            self.set_extreme(t, LEFT, ax, el, msel);
            self.set_extreme(t, RIGHT, ax, er, mser);

            for c in c0..c0 + k {
                let target = self.prelim[t][b] + (self.ext[t][b] - self.ext[c][b]) / 2.0;
                let off = target - (self.prelim[c][b] + self.modifier[c][b]);
                self.modifier[c][b] += off;
                self.msel[c][b] += off;
                self.mser[c][b] += off;
            }

            let mut d1 = f64::NEG_INFINITY;
            let mut j1 = 0;
            for i in 0..k {
                let m = self.maxdepth[c0 + i];
                if m > d1 {
                    d1 = m;
                    j1 = i;
                }
            }

            if k == 1 {
                let (el, msel) = (self.el[c0][b], self.msel[c0][b]);
                let (er, mser) = (self.er[c0][b], self.mser[c0][b]);
                self.set_extreme(t, LEFT, b, el, msel);
                self.set_extreme(t, RIGHT, b, er, mser);
            } else {
                let mut d2 = f64::NEG_INFINITY;
                for i in 0..k {
                    let m = self.maxdepth[c0 + i];
                    if i != j1 && m > d2 {
                        d2 = m;
                    }
                }
                self.hull_contour(t, b, LEFT, j1, d2);
                self.hull_contour(t, b, RIGHT, j1, d2);
            }

            let mut bbox = [
                0.0,
                self.ext[t][0] + self.margin[t],
                0.0,
                self.ext[t][1] + self.margin[t],
            ];
            let origin = self.prelim[t];
            let (mut d, mut modsumdelta) = (0.0, 0.0);
            for c in c0..c0 + k {
                d += self.shift[c];
                modsumdelta += d + self.change[c];
                let mut rel = [
                    self.prelim[c][0] + self.modifier[c][0] - origin[0],
                    self.prelim[c][1] + self.modifier[c][1] - origin[1],
                ];
                rel[ax] += modsumdelta;
                for q in 0..2 {
                    let (lo, hi) = (
                        rel[q] + self.bbox[c][2 * q],
                        rel[q] + self.bbox[c][2 * q + 1],
                    );
                    if lo < bbox[2 * q] {
                        bbox[2 * q] = lo;
                    }
                    if hi > bbox[2 * q + 1] {
                        bbox[2 * q + 1] = hi;
                    }
                }
            }

            let bottom = self.bot[t];
            self.bbox[t] = bbox;
            self.maxdepth[t] = if d1 > bottom { d1 } else { bottom };
        }

        self.chain = chain;
    }

    /// `second_walk`: a forward sweep, so every parent is done before its children, and
    /// the child spacing lands in their modifiers before they are read.
    fn second_sweep(&mut self, frame: Frame, centered: bool) -> [f64; 2] {
        let mut best = [f64::INFINITY; 2];

        for v in 0..self.n {
            let init = if self.parent[v] == NONE {
                [0.0, 0.0]
            } else {
                self.modsum[self.parent[v]]
            };
            let modsum = [init[0] + self.modifier[v][0], init[1] + self.modifier[v][1]];
            self.modsum[v] = modsum;

            let d0 = self.prelim[v][0] + modsum[0];
            let d1 = self.prelim[v][1] + modsum[1];
            let (o0, od, o1) = if centered {
                (
                    self.ext[v][0] / 2.0,
                    self.dext[v] / 2.0,
                    self.ext[v][1] / 2.0,
                )
            } else {
                (0.0, 0.0, 0.0)
            };

            let b = [d0 + o0, d1 + o1];
            let c = &mut self.coord[v];
            c[frame.breadth[0]] = b[0];
            c[frame.breadth[1]] = b[1];
            c[frame.depth] = self.top[v] + od;

            for q in 0..2 {
                if b[q] < best[q] {
                    best[q] = b[q];
                }
            }

            // add_child_spacing
            let ax = self.fam[v];
            let (mut d, mut modsumdelta) = (0.0, 0.0);
            for c in self.children(v) {
                d += self.shift[c];
                modsumdelta += d + self.change[c];
                self.modifier[c][ax] += modsumdelta;
            }
        }

        best
    }

    fn write_back(&self, arena: &mut Arena3, centered: bool, frame: Frame) {
        for v in 0..self.n {
            let node = &mut arena[self.id[v]];
            [node.x, node.y, node.z] = self.coord[v];
            node.level = self.level[v];
            node.centered = centered;
            node.fam = self.fam[v];
            node.family = Axis::of(frame.breadth[self.fam[v]]);

            // The recursion fills these in for children only, leaving whatever the root
            // already carried; do the same.
            if self.parent[v] != NONE {
                node.parent = Some(self.id[self.parent[v]]);
                node.childno = self.childno[v];
            }
        }
    }

    /// Writes `[x_0.., y_0.., z_0..]` over the first `3n` of `whd`.
    fn write_back_arrays(&self, n: usize, whd: &mut [f64]) {
        // A node the root cannot reach is left at the origin, as `layout_api3` leaves it.
        if self.content < n {
            for v in whd[..3 * n].iter_mut() {
                *v = 0.0;
            }
        }

        for v in 0..self.n {
            let s = self.src[v];
            if s < n {
                let [x, y, z] = self.coord[v];
                whd[s] = x;
                whd[s + n] = y;
                whd[s + 2 * n] = z;
            }
        }
    }
}

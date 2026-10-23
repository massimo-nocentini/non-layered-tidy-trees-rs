//! The 3D layout over a struct-of-arrays mirror, laid out by sweeps instead of recursion.
//!
//! [`layout3_flat`] computes exactly what [`super::layout3`] computes, bit for bit, the way
//! [`crate::flat`] does for the 2D layout: the tree is copied into a mirror where the
//! children of a node are a contiguous range and a parent always comes before its children;
//! the walks become forward or backward sweeps over that mirror; and nothing recurses, so
//! the depth of a tree is bounded by memory and not by the stack.
//!
//! The order of the mirror is the one that reads its source in sequence: depth-first with
//! every family pushed as one block from an arena, which is usually built in preorder, and
//! breadth-first from the arrays of the flat API, which are usually numbered that way. The
//! coordinates do not depend on it.
//!
//! The mirror is laid out for the contour walk, which is where the time goes. Everything
//! one step of it reads about a box — its bottom, `prelim`, `modifier`, extents and margin
//! — is one 64-byte `Band`, a cache line; what it reads about a node to find the next
//! step — the children, the family axis, the threads — is one `Link`. The rest is in
//! arrays of its own, which the walk does not touch.
//!
//! [`layout_api3_flat`] fills the mirror straight from the arrays of [`super::layout_api3`],
//! and folds the gap node above every node into the node's own entry, as [`crate::flat`]
//! does: the gap node is the entry's upper band, with its own extents and no margin, and
//! shares everything else. That is sound because a gap node has exactly one child: along
//! both breadth axes its contours continue straight into the node below, it never builds
//! a hull, and the extremes, the subtree's extent and its depth are the node's. The
//! mirror holds one entry per node instead of two.
//!
//! A contour item is an index: band `b` below `bands`, then `bands + 2v` and
//! `bands + 2v + 1` for the left and right hull of entry `v`.
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
        self.m.resolve(input, frame);
        sum += phase!(t, "build", "nodes={}", self.m.n);

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

/// What one step of the contour walk reads about a box: a cache line.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, align(64))]
struct Band {
    bot: f64,
    prelim: [f64; 2],
    modifier: [f64; 2],
    ext: [f64; 2],
    margin: f64,
}

/// What the contour walk reads about a node to find its next step.
#[derive(Debug, Clone, Copy)]
struct Link {
    first: usize,
    count: u32,
    fam: u32,
    tl: [usize; 2],
    tr: [usize; 2],
}

const LEAF: Link = Link {
    first: 0,
    count: 0,
    fam: 0,
    tl: [NONE; 2],
    tr: [NONE; 2],
};

/// The extreme items of a subtree, and the modifier sums down to them.
#[derive(Debug, Clone, Copy)]
struct Extremes {
    el: [usize; 2],
    er: [usize; 2],
    msel: [f64; 2],
    mser: [f64; 2],
}

const NO_EXTREMES: Extremes = Extremes {
    el: [NONE; 2],
    er: [NONE; 2],
    msel: [0.0; 2],
    mser: [0.0; 2],
};

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

/// The tree, transposed, in the frame of the layout: the breadth axes indexed 0 and 1.
///
/// Arrays of bands hold one value per box: one per entry, or two when [`Mirror::paired`].
/// Every other array holds one value per entry.
#[derive(Debug, Default)]
struct Mirror {
    /// Entries.
    n: usize,
    /// Whether every entry carries the gap node of the flat API as its upper band, band
    /// `2v`, with the node itself in band `2v + 1`.
    paired: bool,

    band: Vec<Band>,
    /// Per band, read by the depth and second sweeps only: `[top, extent]` along the
    /// depth axis, and the modifier sums.
    depth: Vec<[f64; 2]>,
    modsum: Vec<[f64; 2]>,

    link: Vec<Link>,
    ex: Vec<Extremes>,
    /// `[shift, change]`, along the family axis of the parent.
    spread: Vec<[f64; 2]>,
    maxdepth: Vec<f64>,
    bbox: Vec<[f64; 4]>,
    /// Two per entry, left then right.
    hulls: Vec<Hull>,
    /// `x`, `y`, `z` of the node itself.
    coord: Vec<[f64; 3]>,

    parent: Vec<usize>,
    childno: Vec<usize>,
    level: Vec<usize>,
    /// What each node asks for, when the mirror comes from an arena.
    axis: Vec<Option<Axis>>,

    /// Where each entry came from, when the mirror was built from an arena.
    id: Vec<NodeId>,
    /// Which node of the flat API each entry is.
    src: Vec<usize>,
    /// How many nodes of the flat API the mirror reached.
    content: usize,

    chain: Vec<ChainLink>,
    height: Vec<usize>,
    stack: Vec<usize>,
    /// Where each node of the arena is in the mirror, for the write back.
    pos: Vec<usize>,
}

/// `v` becomes `n` copies of `zero`, keeping whatever it already allocated.
fn reset<T: Copy>(v: &mut Vec<T>, n: usize, zero: T) {
    v.clear();
    if v.capacity() < n {
        v.reserve_exact(n);
    }
    v.resize(n, zero);
}

impl Mirror {
    /// The band an entry presents to its parent: its gap node, where there is one.
    #[inline(always)]
    fn top_band(&self, v: usize) -> usize {
        if self.paired {
            2 * v
        } else {
            v
        }
    }

    /// The band of the node itself.
    #[inline(always)]
    fn con(&self, v: usize) -> usize {
        if self.paired {
            2 * v + 1
        } else {
            v
        }
    }

    #[inline(always)]
    fn bands(&self) -> usize {
        if self.paired {
            2 * self.n
        } else {
            self.n
        }
    }

    fn clear_shape(&mut self) {
        self.band.clear();
        self.depth.clear();
        self.link.clear();
        self.parent.clear();
        self.childno.clear();
        self.level.clear();
        self.axis.clear();
        self.id.clear();
        self.src.clear();
    }

    fn band_of(world: [f64; 3], margin: f64, frame: Frame) -> (Band, [f64; 2]) {
        let band = Band {
            bot: 0.0,
            prelim: [0.0; 2],
            modifier: [0.0; 2],
            ext: [world[frame.breadth[0]], world[frame.breadth[1]]],
            margin,
        };
        (band, [0.0, world[frame.depth]])
    }

    fn push_entry(&mut self, parent: usize, childno: usize) {
        self.link.push(LEAF);
        self.parent.push(parent);
        self.childno.push(childno);
        self.level.push(if parent == NONE {
            0
        } else {
            self.level[parent] + 1
        });
    }

    fn push_band(&mut self, world: [f64; 3], margin: f64, frame: Frame) {
        let (band, depth) = Mirror::band_of(world, margin, frame);
        self.band.push(band);
        self.depth.push(depth);
    }

    /// Fills the mirror from the arena.
    fn rebuild(&mut self, arena: &Arena3, root: NodeId, frame: Frame) {
        self.clear_shape();
        self.paired = false;

        let add = |m: &mut Mirror, id: NodeId, parent: usize, childno: usize| {
            let node = &arena[id];
            m.push_band([node.w, node.h, node.d], node.margin, frame);
            m.push_entry(parent, childno);
            m.axis.push(node.axis);
            m.id.push(id);
        };

        add(self, root, NONE, 0);

        // Depth-first, every node's children pushed as one block when the node is reached:
        // the children stay a contiguous range after their parent, and the reads follow an
        // arena built in preorder instead of jumping across it level by level.
        let mut stack = std::mem::take(&mut self.stack);
        stack.clear();
        stack.push(0);
        while let Some(i) = stack.pop() {
            let children = arena.children(self.id[i]);
            let first = self.id.len();
            self.link[i].first = first;
            self.link[i].count = children.len() as u32;
            for (k, &c) in children.iter().enumerate() {
                add(self, c, i, k);
            }
            stack.extend((first..first + children.len()).rev());
        }
        self.stack = stack;

        self.n = self.id.len();
        self.content = self.n;
        self.reset_state();
    }

    /// Fills the mirror from the arrays of the flat API, breadth-first, every node and its
    /// gap node one entry.
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
        self.paired = true;

        let add = |m: &mut Mirror, s: usize, parent: usize, childno: usize| {
            m.push_band([gaps[s], gaps[s + n], gaps[s + 2 * n]], 0.0, frame);
            m.push_band([whd[s], whd[s + n], whd[s + 2 * n]], whd[s + 3 * n], frame);
            m.push_entry(parent, childno);
            m.src.push(s);
        };

        add(self, rooti - 1, NONE, 0);

        let mut e = 0;
        while e < self.src.len() {
            let s = self.src[e];
            self.link[e].first = self.src.len();
            self.link[e].count = children[s] as u32;
            for k in 0..children[s] {
                add(self, children[off[s] + k] - 1, e, k);
            }

            // The family axes go by the heights of the tree without its gap nodes, as
            // `reify3` sets them; a gap node shares the axis of the node below it.
            self.link[e].fam = if children[s] == 0 {
                0
            } else {
                frame.family(None, input.rule, height[s]) as u32
            };
            e += 1;
        }

        self.n = self.src.len();
        self.content = self.n;
        self.reset_state();
    }

    fn reset_state(&mut self) {
        let (n, b) = (self.n, self.bands());
        reset(&mut self.modsum, b, [0.0; 2]);
        reset(&mut self.ex, n, NO_EXTREMES);
        reset(&mut self.spread, n, [0.0; 2]);
        reset(&mut self.maxdepth, n, 0.0);
        reset(&mut self.bbox, n, [0.0; 4]);
        reset(&mut self.hulls, 2 * n, EMPTY_HULL);
        reset(&mut self.coord, n, [0.0; 3]);
    }

    /// The family axes, by the nodes' own axes and the rule over the subtree heights: a
    /// backward sweep, children before parents.
    fn resolve(&mut self, input: &LayoutInput3, frame: Frame) {
        let mut height = std::mem::take(&mut self.height);
        reset(&mut height, self.n, 0);

        for v in (0..self.n).rev() {
            let l = self.link[v];
            if l.count == 0 {
                continue;
            }
            let mut h = 0;
            for &hc in &height[l.first..l.first + l.count as usize] {
                if hc + 1 > h {
                    h = hc + 1;
                }
            }
            height[v] = h;
            self.link[v].fam = frame.family(self.axis[v], input.rule, h) as u32;
        }

        self.height = height;
    }

    /// The three sweeps and the normalization; what the traced phases added up to.
    fn sweeps(&mut self, frame: Frame, centered: bool, origin: [f64; 3]) -> std::time::Duration {
        let mut sum = std::time::Duration::ZERO;

        let t = crate::trace::start();
        self.setup_sweep();
        sum += phase!(t, "setup");

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

    /// `setup_walk`'s depth axis: every box starts at the bottom of the one above it.
    fn setup_sweep(&mut self) {
        for v in 0..self.n {
            let p = self.parent[v];
            let mut top = if p == NONE {
                0.0
            } else {
                self.band[self.con(p)].bot
            };
            if self.paired {
                let g = 2 * v;
                self.depth[g][0] = top;
                self.band[g].bot = top + self.depth[g][1];
                top = self.band[g].bot;
            }
            let c = self.con(v);
            self.depth[c][0] = top;
            self.band[c].bot = top + self.depth[c][1];
        }
    }

    #[inline(always)]
    fn item_bottom(&self, it: usize) -> f64 {
        let nb = self.bands();
        if it < nb {
            self.band[it].bot
        } else {
            self.hulls[it - nb].bottom
        }
    }

    #[inline(always)]
    fn item_modifier(&self, it: usize, ax: usize) -> f64 {
        let nb = self.bands();
        if it < nb {
            self.band[it].modifier[ax]
        } else {
            self.hulls[it - nb].modifier
        }
    }

    #[inline(always)]
    fn near(&self, sum: f64, it: usize, ax: usize) -> f64 {
        let nb = self.bands();
        if it < nb {
            sum + self.band[it].prelim[ax]
        } else {
            sum + self.hulls[it - nb].prelim
        }
    }

    #[inline(always)]
    fn far(&self, sum: f64, it: usize, ax: usize) -> f64 {
        let nb = self.bands();
        if it < nb {
            let b = &self.band[it];
            sum + b.prelim[ax] + b.ext[ax] + b.margin
        } else {
            sum + self.hulls[it - nb].prelim
        }
    }

    fn nudge(&mut self, it: usize, ax: usize, diff: f64) {
        let nb = self.bands();
        if it < nb {
            let b = &mut self.band[it];
            b.modifier[ax] += diff;
            b.prelim[ax] -= diff;
        } else {
            let h = &mut self.hulls[it - nb];
            h.modifier += diff;
            h.prelim -= diff;
        }
    }

    /// Threads only ever start at a leaf's own box or at a hull.
    fn set_thread(&mut self, it: usize, side: usize, ax: usize, to: usize) {
        let nb = self.bands();
        if it < nb {
            let v = if self.paired { it >> 1 } else { it };
            if side == LEFT {
                self.link[v].tl[ax] = to;
            } else {
                self.link[v].tr[ax] = to;
            }
        } else {
            self.hulls[it - nb].thread = to;
        }
    }

    #[inline(always)]
    fn next(&self, it: usize, side: usize, ax: usize) -> usize {
        let nb = self.bands();
        if it >= nb {
            return self.hulls[it - nb].thread;
        }
        let v = if self.paired {
            if it & 1 == 0 {
                // a gap band: its only child is the node below it, along either axis
                return it + 1;
            }
            it >> 1
        } else {
            it
        };
        let l = &self.link[v];
        let count = l.count as usize;
        if count == 0 {
            if side == LEFT {
                l.tl[ax]
            } else {
                l.tr[ax]
            }
        } else if l.fam as usize == ax {
            if side == LEFT {
                self.top_band(l.first)
            } else {
                self.top_band(l.first + count - 1)
            }
        } else if count == 1 {
            self.top_band(l.first)
        } else {
            nb + 2 * v + side
        }
    }

    fn move_subtree(&mut self, t: usize, i: usize, si: usize, dist: f64, ax: usize) {
        let base = self.link[t].first;
        let ci = base + i;
        let tb = self.top_band(ci);
        self.band[tb].modifier[ax] += dist;
        self.ex[ci].msel[ax] += dist;
        self.ex[ci].mser[ax] += dist;

        if si + 1 != i {
            let nr = (i - si) as f64;
            let ratio = dist / nr;
            self.spread[base + si + 1][0] += ratio;
            self.spread[ci][0] -= ratio;
            self.spread[ci][1] -= dist - ratio;
        }
    }

    fn set_left_thread(&mut self, t: usize, i: usize, cl: usize, modsumcl: f64, ax: usize) {
        let c0 = self.link[t].first;
        let ci = c0 + i;
        let li = self.ex[c0].el[ax];

        let diff = (modsumcl - self.item_modifier(cl, ax)) - self.ex[c0].msel[ax];

        self.set_thread(li, LEFT, ax, cl);
        self.nudge(li, ax, diff);

        self.ex[c0].el[ax] = self.ex[ci].el[ax];
        self.ex[c0].msel[ax] = self.ex[ci].msel[ax];
    }

    fn set_right_thread(&mut self, t: usize, i: usize, sr: usize, modsumsr: f64, ax: usize) {
        let ci = self.link[t].first + i;
        let cp = ci - 1;
        let ri = self.ex[ci].er[ax];
        let diff = (modsumsr - self.item_modifier(sr, ax)) - self.ex[ci].mser[ax];

        self.set_thread(ri, RIGHT, ax, sr);
        self.nudge(ri, ax, diff);

        self.ex[ci].er[ax] = self.ex[cp].er[ax];
        self.ex[ci].mser[ax] = self.ex[cp].mser[ax];
    }

    fn separate(&mut self, t: usize, i: usize, chain: &[ChainLink], ax: usize) {
        let base = self.link[t].first;
        let mut sr = self.top_band(base + i - 1);
        let mut mssr = self.band[sr].modifier[ax];
        let mut cl = self.top_band(base + i);
        let mut mscl = self.band[cl].modifier[ax];

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
            self.ex[t].el[ax] = it;
            self.ex[t].msel[ax] = ms;
        } else {
            self.ex[t].er[ax] = it;
            self.ex[t].mser[ax] = ms;
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

        let base = self.link[t].first;
        let deep = base + j1;
        let mut it = self.top_band(deep);
        let mut s = self.band[it].modifier[b];
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

        for j in 0..self.link[t].count as usize {
            if j == j1 {
                continue;
            }
            let c = base + j;
            let band = &self.band[self.top_band(c)];
            let rel = self.bbox[c][2 * b + side];
            ext = extreme(ext, band.prelim[b] + band.modifier[b] + rel);
        }

        let hull = match tee {
            Some((tee, st)) => {
                let m = st - self.item_modifier(tee, b);
                let (el, ms) = if side == LEFT {
                    (self.ex[deep].el[b], self.ex[deep].msel[b])
                } else {
                    (self.ex[deep].er[b], self.ex[deep].mser[b])
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
                self.set_extreme(t, side, b, self.bands() + 2 * t + side, 0.0);
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

    /// `first_walk`: a backward sweep, so every child is done before its parent; for a
    /// paired mirror, the gap band right after the node below it.
    fn first_sweep(&mut self) {
        let mut chain = std::mem::take(&mut self.chain);

        for t in (0..self.n).rev() {
            self.first_node(t, &mut chain);
            if self.paired {
                self.first_gap(t);
            }
        }

        self.chain = chain;
    }

    /// `first_walk` of the node of entry `t`, whose children present their top bands.
    fn first_node(&mut self, t: usize, chain: &mut Vec<ChainLink>) {
        let tb = self.con(t);
        let l = self.link[t];
        let k = l.count as usize;

        if k == 0 {
            self.ex[t] = Extremes {
                el: [tb; 2],
                er: [tb; 2],
                msel: [0.0; 2],
                mser: [0.0; 2],
            };
            let b = &self.band[tb];
            self.bbox[t] = [0.0, b.ext[0] + b.margin, 0.0, b.ext[1] + b.margin];
            self.maxdepth[t] = b.bot;
            return;
        }

        let ax = l.fam as usize;
        let b = 1 - ax;
        let c0 = l.first;
        let last = c0 + k - 1;

        chain.clear();
        update_chain(chain, self.item_bottom(self.ex[c0].el[ax]), 0);

        for i in 1..k {
            let min = self.item_bottom(self.ex[c0 + i].er[ax]);
            self.separate(t, i, chain, ax);
            update_chain(chain, min, i);
        }

        // position_root
        let (f, lb) = (self.band[self.top_band(c0)], self.band[self.top_band(last)]);
        let d = lb.ext[ax] - self.band[tb].ext[ax];
        self.band[tb].prelim[ax] =
            (f.prelim[ax] + f.modifier[ax] + lb.prelim[ax] + lb.modifier[ax] + d) / 2.0;

        let (el, msel) = (self.ex[c0].el[ax], self.ex[c0].msel[ax]);
        let (er, mser) = (self.ex[last].er[ax], self.ex[last].mser[ax]);
        self.set_extreme(t, LEFT, ax, el, msel);
        self.set_extreme(t, RIGHT, ax, er, mser);

        for c in c0..c0 + k {
            let cb = self.top_band(c);
            let target =
                self.band[tb].prelim[b] + (self.band[tb].ext[b] - self.band[cb].ext[b]) / 2.0;
            let band = &mut self.band[cb];
            let off = target - (band.prelim[b] + band.modifier[b]);
            band.modifier[b] += off;
            self.ex[c].msel[b] += off;
            self.ex[c].mser[b] += off;
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
            let (el, msel) = (self.ex[c0].el[b], self.ex[c0].msel[b]);
            let (er, mser) = (self.ex[c0].er[b], self.ex[c0].mser[b]);
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

        let own = self.band[tb];
        let mut bbox = [0.0, own.ext[0] + own.margin, 0.0, own.ext[1] + own.margin];
        let origin = own.prelim;
        let (mut d, mut modsumdelta) = (0.0, 0.0);
        for c in c0..c0 + k {
            d += self.spread[c][0];
            modsumdelta += d + self.spread[c][1];
            let band = &self.band[self.top_band(c)];
            let mut rel = [
                band.prelim[0] + band.modifier[0] - origin[0],
                band.prelim[1] + band.modifier[1] - origin[1],
            ];
            rel[ax] += modsumdelta;
            union(&mut bbox, rel, &self.bbox[c]);
        }

        self.bbox[t] = bbox;
        self.maxdepth[t] = if d1 > own.bot { d1 } else { own.bot };
    }

    /// `first_walk` of the gap node of entry `t`, one child: the node below it. The
    /// extremes along both axes are the node's already, and so is the subtree's depth.
    fn first_gap(&mut self, t: usize) {
        let (g, c) = (2 * t, 2 * t + 1);
        let ax = self.link[t].fam as usize;
        let b = 1 - ax;

        // position_root, with the node as first and last child
        let (gb, cb) = (self.band[g], self.band[c]);
        let d = cb.ext[ax] - gb.ext[ax];
        self.band[g].prelim[ax] =
            (cb.prelim[ax] + cb.modifier[ax] + cb.prelim[ax] + cb.modifier[ax] + d) / 2.0;

        // the node centred under its gap node along the other axis
        let target = self.band[g].prelim[b] + (gb.ext[b] - cb.ext[b]) / 2.0;
        let off = target - (cb.prelim[b] + cb.modifier[b]);
        self.band[c].modifier[b] += off;
        self.ex[t].msel[b] += off;
        self.ex[t].mser[b] += off;

        // the extent: the gap's own box, no margin, and the node's subtree, which nothing
        // spreads -- the same zeros the recursion adds
        let gb = self.band[g];
        let cb = self.band[c];
        let mut bbox = [0.0, gb.ext[0] + 0.0, 0.0, gb.ext[1] + 0.0];
        let d = 0.0 + 0.0;
        let modsumdelta = 0.0 + (d + 0.0);
        let mut rel = [
            cb.prelim[0] + cb.modifier[0] - gb.prelim[0],
            cb.prelim[1] + cb.modifier[1] - gb.prelim[1],
        ];
        rel[ax] += modsumdelta;
        let inner = self.bbox[t];
        union(&mut bbox, rel, &inner);
        self.bbox[t] = bbox;

        let d1 = self.maxdepth[t];
        self.maxdepth[t] = if d1 > gb.bot { d1 } else { gb.bot };
    }

    /// `second_walk`: a forward sweep, so every parent is done before its children, and
    /// the child spacing lands in their modifiers before they are read.
    fn second_sweep(&mut self, frame: Frame, centered: bool) -> [f64; 2] {
        let mut best = [f64::INFINITY; 2];

        for v in 0..self.n {
            let p = self.parent[v];
            let mut init = if p == NONE {
                [0.0, 0.0]
            } else {
                self.modsum[self.con(p)]
            };
            let ax = self.link[v].fam as usize;

            if self.paired {
                // The gap node is drawn nowhere, but it counts towards the normalization,
                // as it does when it is a node of its own.
                let g = 2 * v;
                let (b0, b1) = self.place(g, init, centered);
                least(&mut best, b0, b1);
                init = self.modsum[g];

                let d = 0.0 + 0.0;
                let modsumdelta = 0.0 + (d + 0.0);
                self.band[g + 1].modifier[ax] += modsumdelta;
            }

            let c = self.con(v);
            let (b0, b1) = self.place(c, init, centered);
            least(&mut best, b0, b1);

            let od = if centered {
                self.depth[c][1] / 2.0
            } else {
                0.0
            };
            let coord = &mut self.coord[v];
            coord[frame.breadth[0]] = b0;
            coord[frame.breadth[1]] = b1;
            coord[frame.depth] = self.depth[c][0] + od;

            // add_child_spacing
            let l = self.link[v];
            let (mut d, mut modsumdelta) = (0.0, 0.0);
            for ch in l.first..l.first + l.count as usize {
                d += self.spread[ch][0];
                modsumdelta += d + self.spread[ch][1];
                let tb = self.top_band(ch);
                self.band[tb].modifier[ax] += modsumdelta;
            }
        }

        best
    }

    /// The breadth coordinates of a band, its modifier sum stored on the way.
    fn place(&mut self, b: usize, init: [f64; 2], centered: bool) -> (f64, f64) {
        let band = &self.band[b];
        let modsum = [init[0] + band.modifier[0], init[1] + band.modifier[1]];
        self.modsum[b] = modsum;

        let d0 = band.prelim[0] + modsum[0];
        let d1 = band.prelim[1] + modsum[1];
        let (o0, o1) = if centered {
            (band.ext[0] / 2.0, band.ext[1] / 2.0)
        } else {
            (0.0, 0.0)
        };
        (d0 + o0, d1 + o1)
    }

    fn write_back(&mut self, arena: &mut Arena3, centered: bool, frame: Frame) {
        let mut pos = std::mem::take(&mut self.pos);
        let put = |node: &mut super::Node3, v: usize| {
            [node.x, node.y, node.z] = self.coord[v];
            node.level = self.level[v];
            node.centered = centered;
            node.fam = self.link[v].fam as usize;
            node.family = Axis::of(frame.breadth[node.fam]);

            // The recursion fills these in for children only, leaving whatever the root
            // already carried; do the same.
            if self.parent[v] != NONE {
                node.parent = Some(self.id[self.parent[v]]);
                node.childno = self.childno[v];
            }
        };

        if 2 * self.n >= arena.len() {
            // The tree is most of the arena: write it in the arena's own order, so that
            // the writes stream through the nodes and only the reads jump.
            reset(&mut pos, arena.len(), NONE);
            for (v, id) in self.id.iter().enumerate() {
                pos[id.0] = v;
            }
            for (i, node) in arena.nodes.iter_mut().enumerate() {
                if pos[i] != NONE {
                    put(node, pos[i]);
                }
            }
        } else {
            for v in 0..self.n {
                put(&mut arena.nodes[self.id[v].0], v);
            }
        }

        self.pos = pos;
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
            let [x, y, z] = self.coord[v];
            whd[s] = x;
            whd[s + n] = y;
            whd[s + 2 * n] = z;
        }
    }
}

/// Grows `bbox` by the extent `inner` of a child placed at `rel`.
#[inline(always)]
fn union(bbox: &mut [f64; 4], rel: [f64; 2], inner: &[f64; 4]) {
    for q in 0..2 {
        let (lo, hi) = (rel[q] + inner[2 * q], rel[q] + inner[2 * q + 1]);
        if lo < bbox[2 * q] {
            bbox[2 * q] = lo;
        }
        if hi > bbox[2 * q + 1] {
            bbox[2 * q + 1] = hi;
        }
    }
}

fn least(best: &mut [f64; 2], b0: f64, b1: f64) {
    if b0 < best[0] {
        best[0] = b0;
    }
    if b1 < best[1] {
        best[1] = b1;
    }
}

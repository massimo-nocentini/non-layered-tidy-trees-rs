//! A struct-of-arrays mirror of the tree, laid out by sweeps instead of by recursion.
//!
//! [`layout_flat`] computes exactly what [`crate::layout`] computes — the same arithmetic
//! in the same order, checked bit for bit against the C build over the whole corpus — but
//! it gets there differently:
//!
//! * the tree is copied into a transient mirror in **breadth-first order**, so the children
//!   of a node are a contiguous index range and every node of a given depth is adjacent;
//! * the invisible node [`crate::layout_api`] inserts above every node to space the levels
//!   apart is not a mirror entry of its own: it becomes the upper *band* of the entry it
//!   belongs to, which halves the largest mirror there is. It is a fold and not a merge --
//!   the band keeps the gap node's own width and its lack of a margin, and the contour walk
//!   still compares it separately, which is what keeps the coordinates identical;
//! * every walk becomes a sweep over that mirror, forwards or backwards, with no recursion
//!   at all — which also removes the stack-depth ceiling the recursive path has;
//! * `bottom` is precomputed once into `bot[]`, which takes two branches out of the
//!   innermost contour step;
//! * the elementwise sweeps go through [`Kernels`], which dispatches to AVX2 when the crate
//!   is built with `--features simd` and the CPU has it.
//!
//! This is steps 2 to 4 of `simd-plan.md`, whose verdict is worth repeating: `separate` is
//! the hot routine, it is a serial contour walk, and it cannot be vectorized at any width.
//! What is vectorized here is the minority of the runtime that *is* elementwise. See
//! `rust/README.md` for what that turns out to be worth.
//!
//! # Order of visits
//!
//! A sweep visits independent subtrees in a different order than the recursion does. The
//! arithmetic per node is unchanged, so the coordinates are identical; what is not
//! preserved is the order a `walk` callback would see, which is why this path takes no
//! callbacks. Use [`crate::layout_with`] when you need them.

use crate::{Arena, LayoutInput, NodeId};

/// The absence of a node, where the pointer version has `NULL`.
///
/// `usize::MAX` rather than `-1`: the indices are as wide as the address space, so a mirror
/// is capped by the memory it fits in rather than by 2^31 nodes.
const NONE: usize = usize::MAX;

/// Lays out the tree with the flat sweeps, writing `x` and `y` back into the arena.
///
/// Equivalent to [`crate::layout`] in every observable way except the order in which nodes
/// are visited internally, which no caller can see: it takes no callbacks.
pub fn layout_flat(arena: &mut Arena, input: &LayoutInput) {
    Engine::new().layout(arena, input);
}

/// [`layout_flat`], with the elementwise sweeps vectorized where the CPU allows it.
///
/// Falls back to the scalar kernels when the target has no AVX2, so the result is the same
/// everywhere — the kernels are elementwise, and widening an elementwise operation does not
/// change a single bit of it.
#[cfg(feature = "simd")]
pub fn layout_flat_simd(arena: &mut Arena, input: &LayoutInput) {
    Engine::with_simd().layout(arena, input);
}

/// A reusable mirror, for laying out repeatedly.
///
/// [`layout_flat`] allocates the mirror on every call, which for a large tree is more work
/// than the layout itself: a million nodes is a hundred megabytes of arrays to fault in and
/// hand back. An `Engine` keeps them between calls, so only the first layout pays. The
/// results are identical either way.
///
/// ```
/// use non_layered_tidy_trees::{flat::Engine, Arena, LayoutInput};
///
/// let mut arena = Arena::new();
/// let root = arena.add_node(1, 10.0, 10.0, 0.0, false);
/// let leaf = arena.add_node(2, 10.0, 10.0, 0.0, false);
/// arena.set_children(root, &[leaf]);
///
/// let mut engine = Engine::new();
/// let input = LayoutInput::new(root);
///
/// engine.layout(&mut arena, &input);
/// engine.layout(&mut arena, &input); // no allocation this time
///
/// assert_eq!((arena[leaf].x, arena[leaf].y), (0.0, 10.0));
/// ```
#[derive(Debug, Default)]
pub struct Engine {
    flat: Flat,
    kernels: Kernels,
}

impl Engine {
    /// An engine using the scalar kernels.
    pub fn new() -> Engine {
        Engine {
            flat: Flat::default(),
            kernels: Kernels::scalar(),
        }
    }

    /// An engine using AVX2 where the CPU has it.
    #[cfg(feature = "simd")]
    pub fn with_simd() -> Engine {
        Engine {
            flat: Flat::default(),
            kernels: Kernels::detect(),
        }
    }

    /// Which kernels this engine dispatches to.
    pub fn kernels(&self) -> Kernels {
        self.kernels
    }

    /// Lays out the tree, reusing whatever this engine already allocated.
    pub fn layout(&mut self, arena: &mut Arena, input: &LayoutInput) {
        run(&mut self.flat, arena, input, self.kernels)
    }

    /// [`layout_api_flat`], reusing whatever this engine already allocated.
    #[allow(clippy::too_many_arguments)]
    pub fn layout_api(
        &mut self,
        n: usize,
        wh: &mut [f64],
        whg: &[f64],
        children: &[usize],
        rooti: usize,
        vertically: bool,
        centeredxy: bool,
        x: f64,
        y: f64,
    ) {
        assert!(
            (1..=n).contains(&rooti),
            "`rooti` is a one-based index into {n} nodes"
        );

        let on = crate::trace::enabled();
        let mut sum = std::time::Duration::ZERO;

        trace!(
            "layout_api_flat  n={n} rooti={rooti} vertically={vertically} centeredxy={centeredxy} origin=({x}, {y}) kernels={}",
            kernel_name(self.kernels)
        );

        let t = crate::trace::start();
        self.flat
            .rebuild_from_arrays(n, wh, whg, children, rooti, vertically);
        sum += phase!(t, "build", "{}", mirror(&self.flat));

        let (minbreadth, swept) =
            sweeps(&mut self.flat, vertically, centeredxy, x, y, self.kernels);
        let _ = minbreadth;
        sum += swept;

        let t = crate::trace::start();
        self.flat.write_back_arrays(n, wh, vertically);
        sum += phase!(t, "write", "{n} coordinate pairs");

        total!(on, sum);
    }

    /// [`Engine::layout`], reporting how long each sweep took.
    ///
    /// A benchmarking aid: `simd-plan.md` asks for the share of runtime each phase holds,
    /// and this is what answers it. See `src/bin/bench.rs`.
    pub fn profile(&mut self, arena: &mut Arena, input: &LayoutInput) -> Phases {
        let mut phases = Phases::default();
        run_profiled(&mut self.flat, arena, input, self.kernels, &mut phases);
        phases
    }
}

/// How long each phase of a flat layout took; see [`Engine::profile`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phases {
    /// Building the mirror: allocation, and one copy in.
    pub build: std::time::Duration,
    /// The depth axis sweep.
    pub setup: std::time::Duration,
    /// The contour walk, which is the algorithm proper.
    pub first: std::time::Duration,
    /// The modifier sums, the coordinates and the child spacing.
    pub second: std::time::Duration,
    /// Normalizing the drawing to the origin.
    pub third: std::time::Duration,
    /// Copying the coordinates back into the arena.
    pub write_back: std::time::Duration,
}

impl Phases {
    /// The phases as `(name, duration)`, in the order they run.
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, std::time::Duration)> {
        [
            ("build", self.build),
            ("setup", self.setup),
            ("first", self.first),
            ("second", self.second),
            ("third", self.third),
            ("write", self.write_back),
        ]
        .into_iter()
    }

    /// The sum of the phases.
    pub fn total(&self) -> std::time::Duration {
        self.iter().map(|(_, d)| d).sum()
    }
}

fn run(flat: &mut Flat, arena: &mut Arena, input: &LayoutInput, k: Kernels) {
    let on = crate::trace::enabled();
    let mut sum = std::time::Duration::ZERO;

    trace!(
        "layout_flat  root=#{} arena={} vertically={} centeredxy={} origin=({}, {}) kernels={}",
        arena[input.root].idx,
        arena.len(),
        input.vertically,
        input.centeredxy,
        input.x,
        input.y,
        kernel_name(k)
    );

    let t = crate::trace::start();
    flat.rebuild(arena, input.root, input.vertically);
    sum += phase!(t, "build", "{}", mirror(flat));

    sum += sweeps(
        flat,
        input.vertically,
        input.centeredxy,
        input.x,
        input.y,
        k,
    )
    .1;

    let t = crate::trace::start();
    flat.write_back(arena, input.vertically, input.centeredxy);
    sum += phase!(t, "write", "{} nodes", flat.n);

    total!(on, sum);
}

/// Which kernels a trace line is about.
fn kernel_name(k: Kernels) -> &'static str {
    if k.is_simd() {
        "avx2"
    } else {
        "scalar"
    }
}

/// The shape of the mirror, for the trace.
fn mirror(flat: &Flat) -> String {
    format!("nodes={} depth={}", flat.n, flat.levels.len() - 1)
}

/// The three sweeps and the normalization, over a mirror that is already filled.
///
/// Returns the minimum breadth coordinate, and what the traced phases added up to — zero
/// when tracing is off; see [`crate::trace`].
fn sweeps(
    flat: &mut Flat,
    vertically: bool,
    centeredxy: bool,
    x: f64,
    y: f64,
    k: Kernels,
) -> (f64, std::time::Duration) {
    let mut sum = std::time::Duration::ZERO;

    let t = crate::trace::start();
    flat.setup_sweep(k);
    sum += phase!(t, "setup");

    let t = crate::trace::start();
    flat.first_sweep();
    sum += phase!(t, "first");

    let t = crate::trace::start();
    let minbreadth = flat.second_sweep(centeredxy, k);
    sum += phase!(t, "second", "minbreadth={minbreadth}");

    // The counterpart of `third_walk`: the depth axis is offset by the input alone, the
    // breadth axis is normalized to the origin first.
    let (in_breadth, in_depth) = if vertically { (x, y) } else { (y, x) };

    let dbreadth = minbreadth - in_breadth;
    let ddepth = -in_depth;

    let t = crate::trace::start();
    if dbreadth != 0.0 || ddepth != 0.0 {
        flat.third_sweep(dbreadth, ddepth, k);
        sum += phase!(t, "third", "dbreadth={dbreadth} ddepth={ddepth}");
    } else {
        sum += phase!(t, "third", "already at the origin");
    }

    (minbreadth, sum)
}

/// [`crate::layout_api`], without ever building a tree of nodes.
///
/// The flat API is handed its tree as arrays already, so the mirror can be filled straight
/// from them: no [`Arena`], no `Node`, no per-node child list, and no copy back through
/// one. This is the case the struct-of-arrays rewrite was worth doing for -- see the
/// benchmark table in `rust/README.md`.
///
/// The arguments are exactly [`crate::layout_api`]'s, and so is the result, bit for bit.
///
/// # Panics
///
/// If the arrays are shorter than the layouts [`crate::layout_api`] documents, or if
/// `rooti` is out of range.
#[allow(clippy::too_many_arguments)]
pub fn layout_api_flat(
    n: usize,
    wh: &mut [f64],
    whg: &[f64],
    children: &[usize],
    rooti: usize,
    vertically: bool,
    centeredxy: bool,
    x: f64,
    y: f64,
) {
    Engine::new().layout_api(n, wh, whg, children, rooti, vertically, centeredxy, x, y);
}

/// [`run`], with a stopwatch between the phases; see [`Engine::profile`].
fn run_profiled(
    flat: &mut Flat,
    arena: &mut Arena,
    input: &LayoutInput,
    k: Kernels,
    phases: &mut Phases,
) {
    use std::time::Instant;

    let t = Instant::now();
    flat.rebuild(arena, input.root, input.vertically);
    phases.build = t.elapsed();

    let t = Instant::now();
    flat.setup_sweep(k);
    phases.setup = t.elapsed();

    let t = Instant::now();
    flat.first_sweep();
    phases.first = t.elapsed();

    let t = Instant::now();
    let minbreadth = flat.second_sweep(input.centeredxy, k);
    phases.second = t.elapsed();

    let (in_breadth, in_depth) = if input.vertically {
        (input.x, input.y)
    } else {
        (input.y, input.x)
    };

    let dbreadth = minbreadth - in_breadth;
    let ddepth = -in_depth;

    let t = Instant::now();
    if dbreadth != 0.0 || ddepth != 0.0 {
        flat.third_sweep(dbreadth, ddepth, k);
    }
    phases.third = t.elapsed();

    let t = Instant::now();
    flat.write_back(arena, input.vertically, input.centeredxy);
    phases.write_back = t.elapsed();

    // the phases are timed here already, so the trace reports those rather than timing
    // them a second time.
    if crate::trace::enabled() {
        crate::trace::header(format_args!(
            "profile      root=#{} arena={} vertically={} centeredxy={} origin=({}, {}) kernels={}",
            arena[input.root].idx,
            arena.len(),
            input.vertically,
            input.centeredxy,
            input.x,
            input.y,
            kernel_name(k)
        ));

        for (name, d) in phases.iter() {
            crate::trace::line(name, d, format_args!(""));
        }

        crate::trace::line("total", phases.total(), format_args!("{}", mirror(flat)));
    }
}

/// The tree, transposed: one array per field, in breadth-first order.
///
/// The breadth axis is the one the algorithm positions along and the depth axis is the one
/// it stacks levels along; which of `x`/`y` each is depends on `vertically`, and the
/// mapping happens once on the way in and once on the way out rather than at every use.
///
/// Every index here is a `usize`, so a mirror is bounded by the memory it fits in and not
/// by a 32-bit node count. That is not free -- the contour walk is memory bound, and eight
/// byte links cost it a quarter of its time against four byte ones -- but a truncating
/// index would be silently wrong rather than slow.
#[derive(Debug, Default)]
struct Flat {
    /// How many entries the mirror holds; `2 * n` bands when [`Flat::paired`].
    n: usize,

    /// Whether every entry carries the gap node of the flat API as a second band.
    ///
    /// The API inserts an invisible node above every node to space the levels apart, which
    /// doubles a tree that is already the largest thing in memory. Here that node is not an
    /// entry of its own: it is the *upper band* of the entry it belongs to, band `2 * v`,
    /// with the node itself in the lower band, `2 * v + 1`. The two bands differ in extent,
    /// in depth and in margin -- a gap node keeps none, which is what a fold into the
    /// node's own box would get wrong -- and share everything the algorithm carries per
    /// subtree: `el`, `er`, `msel`, `mser`, the threads, the children and the parent.
    ///
    /// The band arrays are `bext`, `dext`, `bot`, `prelim` and `modifier`; everything else
    /// is one value per entry. A mirror built from an [`Arena`] has no gap nodes, so there
    /// every entry is one band and the two indices coincide.
    paired: bool,

    /// Extent along the breadth axis: `w` when laying out vertically. Per band.
    bext: Vec<f64>,
    /// Extent along the depth axis: `h` when laying out vertically. Per band.
    dext: Vec<f64>,
    /// Per entry: the band of the gap node keeps no margin of its own.
    margin: Vec<f64>,

    breadth: Vec<f64>,
    depth: Vec<f64>,
    /// `bottom()`, precomputed: `depth + dext` while the walks need it. Per band.
    bot: Vec<f64>,

    /// Per band.
    prelim: Vec<f64>,
    /// Per band.
    modifier: Vec<f64>,
    shift: Vec<f64>,
    change: Vec<f64>,
    msel: Vec<f64>,
    mser: Vec<f64>,
    modsum: Vec<f64>,

    tl: Vec<usize>,
    tr: Vec<usize>,
    el: Vec<usize>,
    er: Vec<usize>,

    parent: Vec<usize>,
    childno: Vec<usize>,
    first_child: Vec<usize>,
    child_count: Vec<usize>,

    /// Level `l` spans `levels[l]..levels[l + 1]`.
    levels: Vec<usize>,
    /// Where each node came from, when the mirror was built from an [`Arena`].
    id: Vec<NodeId>,
    /// Which node of the flat API each entry is, when the mirror was built from arrays.
    src: Vec<usize>,
    /// Where each node's children start in the adjacency; scratch for the flat arrays.
    off: Vec<usize>,
    /// How many content nodes the mirror reached.
    content: usize,
}

/// `v` becomes `n` copies of `zero`, keeping whatever it already allocated.
///
/// The growth `resize` would do on its own doubles, which for the largest trees is up to
/// half the mirror in slack; ask for what is needed and no more.
fn reset<T: Copy>(v: &mut Vec<T>, n: usize, zero: T) {
    v.clear();
    if v.capacity() < n {
        v.reserve_exact(n);
    }
    v.resize(n, zero);
}

/// Room for `n` elements in an empty `v`, without the doubling.
fn reserve_exact<T>(v: &mut Vec<T>, n: usize) {
    if v.capacity() < n {
        v.reserve_exact(n);
    }
}

impl Flat {
    /// Fills the mirror from the arena, reusing every allocation it already holds.
    fn rebuild(&mut self, arena: &Arena, root: NodeId, vertically: bool) {
        let f = self;

        f.paired = false;
        f.id.clear();
        f.src.clear();
        f.bext.clear();
        f.dext.clear();
        f.margin.clear();
        f.parent.clear();
        f.childno.clear();
        f.first_child.clear();
        f.child_count.clear();
        f.levels.clear();
        f.levels.extend([0, 1]);

        f.push(arena, root, NONE, 0, vertically);

        // Breadth-first, one level at a time: parents are always visited before their
        // children, so the children of a node are the contiguous range they were pushed
        // into and the nodes of a level are adjacent.
        let mut start = 0;
        let mut end = 1;
        while start < end {
            for i in start..end {
                f.first_child[i] = f.id.len();
                let children = arena.children(f.id[i]);
                f.child_count[i] = children.len();
                for (k, &c) in children.iter().enumerate() {
                    f.push(arena, c, i, k, vertically);
                }
            }
            start = end;
            end = f.id.len();
            if start < end {
                f.levels.push(end);
            }
        }

        let n = f.id.len();
        f.n = n;
        f.reset_state(n, n);
    }

    /// Zeroes what the sweeps accumulate into, threads included.
    ///
    /// This is what the recursive path's `setup_walk` spends a whole traversal restoring;
    /// `bands` is `2 * entries` for a [paired](Flat::paired) mirror and `entries` otherwise.
    fn reset_state(&mut self, entries: usize, bands: usize) {
        reset(&mut self.bot, bands, 0.0);
        reset(&mut self.prelim, bands, 0.0);
        reset(&mut self.modifier, bands, 0.0);

        reset(&mut self.breadth, entries, 0.0);
        reset(&mut self.depth, entries, 0.0);
        reset(&mut self.shift, entries, 0.0);
        reset(&mut self.change, entries, 0.0);
        reset(&mut self.msel, entries, 0.0);
        reset(&mut self.mser, entries, 0.0);
        reset(&mut self.modsum, entries, 0.0);
        reset(&mut self.tl, entries, NONE);
        reset(&mut self.tr, entries, NONE);
        reset(&mut self.el, entries, NONE);
        reset(&mut self.er, entries, NONE);
    }

    /// The upper band of an entry: the gap node, where there is one.
    #[inline(always)]
    fn gap<const P: bool>(v: usize) -> usize {
        if P {
            2 * v
        } else {
            v
        }
    }

    /// The lower band of an entry: the node itself.
    #[inline(always)]
    fn con<const P: bool>(v: usize) -> usize {
        if P {
            2 * v + 1
        } else {
            v
        }
    }

    /// The entry a band belongs to.
    #[inline(always)]
    fn owner<const P: bool>(b: usize) -> usize {
        if P {
            b >> 1
        } else {
            b
        }
    }

    /// The margin of a band; a gap node keeps none.
    #[inline(always)]
    fn margin_of<const P: bool>(&self, b: usize) -> f64 {
        if P && b.is_multiple_of(2) {
            0.0
        } else {
            self.margin[Self::owner::<P>(b)]
        }
    }

    /// Fills the mirror straight from the arrays of the flat API.
    ///
    /// One entry per node, with the gap node the API inserts above it as the entry's upper
    /// band rather than as an entry of its own; see [`Flat::paired`].
    fn rebuild_from_arrays(
        &mut self,
        n: usize,
        wh: &[f64],
        whg: &[f64],
        children: &[usize],
        rooti: usize,
        vertically: bool,
    ) {
        assert!(
            wh.len() >= 3 * n,
            "`wh` holds w, h and margin for {n} nodes"
        );
        assert!(whg.len() >= 2 * n, "`whg` holds w and h for {n} gap nodes");
        assert!(children.len() >= n, "`children` starts with {n} counts");

        let f = self;

        f.paired = true;
        f.id.clear();
        f.src.clear();
        f.bext.clear();
        f.dext.clear();
        f.margin.clear();
        f.parent.clear();
        f.childno.clear();
        f.first_child.clear();
        f.child_count.clear();
        f.levels.clear();
        f.levels.extend([0, 1]);

        // The adjacency follows the `n` counts, grouped by parent.
        f.off.clear();
        f.off.reserve(n);
        let mut acc = n;
        for &count in &children[..n] {
            f.off.push(acc);
            acc += count;
        }
        assert!(
            children.len() >= acc,
            "the adjacency is shorter than the counts promise"
        );

        // Every node of the input is at most one entry, and every entry is two bands;
        // reserving that outright keeps the doubling out of the largest allocations.
        reserve_exact(&mut f.src, n);
        reserve_exact(&mut f.margin, n);
        reserve_exact(&mut f.parent, n);
        reserve_exact(&mut f.childno, n);
        reserve_exact(&mut f.first_child, n);
        reserve_exact(&mut f.child_count, n);
        reserve_exact(&mut f.bext, 2 * n);
        reserve_exact(&mut f.dext, 2 * n);

        f.push_entry(n, rooti - 1, wh, whg, NONE, 0, vertically);

        let mut start = 0;
        let mut end = 1;
        while start < end {
            for m in start..end {
                f.first_child[m] = f.src.len();
                let s = f.src[m];
                let count = children[s];
                f.child_count[m] = count;

                for k in 0..count {
                    let c = children[f.off[s] + k] - 1;
                    f.push_entry(n, c, wh, whg, m, k, vertically);
                }
            }
            start = end;
            end = f.src.len();
            if start < end {
                f.levels.push(end);
            }
        }

        let entries = f.src.len();
        f.n = entries;
        f.content = entries;
        f.reset_state(entries, 2 * entries);
    }

    /// Pushes the entry of node `s`: its gap node into the upper band, itself into the lower.
    #[allow(clippy::too_many_arguments)]
    fn push_entry(
        &mut self,
        n: usize,
        s: usize,
        wh: &[f64],
        whg: &[f64],
        parent: usize,
        childno: usize,
        vertically: bool,
    ) {
        let (w, h) = (wh[s], wh[s + n]);
        let (wg, hg) = (whg[s], whg[s + n]);

        let (bg, dg) = if vertically { (wg, hg) } else { (hg, wg) };
        let (b, d) = if vertically { (w, h) } else { (h, w) };

        self.bext.push(bg);
        self.dext.push(dg);
        self.bext.push(b);
        self.dext.push(d);

        self.src.push(s);
        self.margin.push(wh[s + 2 * n]);
        self.parent.push(parent);
        self.childno.push(childno);
        self.first_child.push(0);
        self.child_count.push(0);
    }

    /// Writes `[x_0.., y_0..]` over the first `2n` of `wh`, as `flat_xy_into` does.
    fn write_back_arrays(&self, n: usize, wh: &mut [f64], vertically: bool) {
        // A node the root cannot reach is not part of the drawing, and the pointer version
        // leaves it at the origin rather than at whatever the input said.
        if self.content < n {
            for v in wh[..2 * n].iter_mut() {
                *v = 0.0;
            }
        }

        for m in 0..self.n {
            let s = self.src[m];
            let (b, d) = (self.breadth[m], self.depth[m]);
            let (x, y) = if vertically { (b, d) } else { (d, b) };

            wh[s] = x;
            wh[s + n] = y;
        }
    }

    fn push(&mut self, arena: &Arena, id: NodeId, parent: usize, childno: usize, vertically: bool) {
        let node = &arena[id];
        let (bext, dext) = if vertically {
            (node.w, node.h)
        } else {
            (node.h, node.w)
        };

        self.id.push(id);
        self.bext.push(bext);
        self.dext.push(dext);
        self.margin.push(node.margin);
        self.parent.push(parent);
        self.childno.push(childno);
        self.first_child.push(0);
        self.child_count.push(0);
    }

    fn children(&self, i: usize) -> std::ops::Range<usize> {
        let f = self.first_child[i];
        f..f + self.child_count[i]
    }

    /// `setup_walk`: the depth axis follows from the node extents, one level below the next.
    fn setup_sweep(&mut self, k: Kernels) {
        if self.paired {
            self.setup_sweep_paired();
        } else {
            self.setup_sweep_plain(k);
        }
    }

    /// [`Flat::setup_sweep`], over a mirror of one band per entry.
    fn setup_sweep_plain(&mut self, k: Kernels) {
        self.depth[0] = 0.0;
        self.bot[0] = self.dext[0];

        for i in 0..self.n {
            let c = self.children(i);
            if c.is_empty() {
                continue;
            }
            let b = self.bot[i];

            // Every child of a node sits directly below it, so this is a fill over a
            // contiguous range and an elementwise add over the same one.
            k.fill(&mut self.depth[c.clone()], b);
            k.add_scalar(&mut self.bot[c.clone()], &self.dext[c], b);
        }
    }

    /// [`Flat::setup_sweep`], over the two bands of every entry.
    ///
    /// One forward pass is enough where the plain sweep needs to reach for its children:
    /// breadth-first order puts a parent before them, so the band a child hangs from is
    /// already final. `depth` is the top of the lower band, which is what the node draws at.
    fn setup_sweep_paired(&mut self) {
        self.bot[0] = self.dext[0];
        self.bot[1] = self.bot[0] + self.dext[1];
        self.depth[0] = self.bot[0];

        for v in 1..self.n {
            let top = self.bot[2 * self.parent[v] + 1];
            let g = 2 * v;

            self.bot[g] = top + self.dext[g];
            self.bot[g + 1] = self.bot[g] + self.dext[g + 1];
            self.depth[v] = self.bot[g];
        }
    }

    /// `first_walk`: a reverse sweep, deepest level first.
    ///
    /// Nodes of one level own disjoint subtrees, so their `separate`/`position_root` work is
    /// independent of one another; visiting them in index order rather than in the order the
    /// recursion would is the only thing that changes. For the same reason a paired mirror
    /// may close an entry's gap band right after the entry rather than a level later.
    fn first_sweep(&mut self) {
        if self.paired {
            self.first_sweep_impl::<true>();
        } else {
            self.first_sweep_impl::<false>();
        }
    }

    fn first_sweep_impl<const P: bool>(&mut self) {
        let mut chain: Vec<ChainLink> = Vec::new();

        for v in (0..self.n).rev() {
            let c = self.children(v);

            if c.is_empty() {
                self.el[v] = v;
                self.er[v] = v;
                self.msel[v] = 0.0;
                self.mser[v] = 0.0;
            } else {
                let first = c.start;
                let last = c.end - 1;

                chain.clear();
                update_chain(&mut chain, self.bot[Self::con::<P>(self.el[first])], 0);

                for k in 1..c.len() {
                    let ci = first + k;
                    let min = self.bot[Self::con::<P>(self.er[ci])];

                    self.separate::<P>(v, k, &chain);

                    update_chain(&mut chain, min, k);
                }

                // `position_root`: between the children, taking their modifiers into
                // account. A child presents its gap band, which is what a parent sees.
                let (t, gf, gl) = (
                    Self::con::<P>(v),
                    Self::gap::<P>(first),
                    Self::gap::<P>(last),
                );
                let d = self.bext[gl] - self.bext[t];
                self.prelim[t] =
                    (self.prelim[gf] + self.modifier[gf] + self.prelim[gl] + self.modifier[gl] + d)
                        / 2.0;

                self.el[v] = self.el[first];
                self.msel[v] = self.msel[first];
                self.er[v] = self.er[last];
                self.mser[v] = self.mser[last];
            }

            if P {
                // The gap node above the entry: one child, so `position_root` and nothing
                // else -- the extremes and their modifier sums are the ones just set.
                let (g, t) = (2 * v, 2 * v + 1);
                let d = self.bext[t] - self.bext[g];

                self.prelim[g] =
                    (self.prelim[t] + self.modifier[t] + self.prelim[t] + self.modifier[t] + d)
                        / 2.0;
            }
        }
    }

    /// The contour walk, which is where the time goes and which stays scalar.
    ///
    /// The two contours are walked band by band: for a paired mirror that is the gap node
    /// and then the node itself, which is exactly what the recursion sees when the two are
    /// separate nodes -- the gap band is the narrower box, and it carries no margin.
    fn separate<const P: bool>(&mut self, t: usize, k: usize, chain: &[ChainLink]) {
        let base = self.first_child[t];
        let i = base + k;

        let mut sr = Self::gap::<P>(i - 1);
        let mut mssr = self.modifier[sr];

        let mut cl = Self::gap::<P>(i);
        let mut mscl = self.modifier[cl];

        let mut ih = chain.len() - 1;
        let mut first = true;

        while sr != NONE && cl != NONE {
            let (s, c) = (sr, cl);

            if self.bot[s] > chain[ih].low {
                ih = ih.checked_sub(1).expect(
                    "the sibling chain is seeded with a value that dominates the subtree depth",
                );
            }

            let dist = (mssr + self.prelim[s] + self.bext[s] + self.margin_of::<P>(s))
                - (mscl + self.prelim[c]);

            if (first && dist < 0.0) || dist > 0.0 {
                mscl += dist;
                self.move_subtree::<P>(t, k, chain[ih].index, dist);
            }

            first = false;

            let (sy, cy) = (self.bot[s], self.bot[c]);

            if sy <= cy {
                sr = self.next_right_contour::<P>(s);
                if sr != NONE {
                    mssr += self.modifier[sr];
                }
            }

            if sy >= cy {
                cl = self.next_left_contour::<P>(c);
                if cl != NONE {
                    mscl += self.modifier[cl];
                }
            }
        }

        if sr == NONE && cl != NONE {
            self.set_left_thread::<P>(base, k, cl, mscl);
        } else if sr != NONE && cl == NONE {
            self.set_right_thread::<P>(base, k, sr, mssr);
        }
    }

    /// The band below `b` on the left contour: the gap node's own band, then a child.
    fn next_left_contour<const P: bool>(&self, b: usize) -> usize {
        if P && b.is_multiple_of(2) {
            return b + 1;
        }

        let v = Self::owner::<P>(b);

        if self.child_count[v] == 0 {
            self.tl[v]
        } else {
            Self::gap::<P>(self.first_child[v])
        }
    }

    /// Symmetrical to [`Flat::next_left_contour`].
    fn next_right_contour<const P: bool>(&self, b: usize) -> usize {
        if P && b.is_multiple_of(2) {
            return b + 1;
        }

        let v = Self::owner::<P>(b);

        if self.child_count[v] == 0 {
            self.tr[v]
        } else {
            Self::gap::<P>(self.first_child[v] + self.child_count[v] - 1)
        }
    }

    fn move_subtree<const P: bool>(&mut self, t: usize, k: usize, si: usize, dist: f64) {
        let base = self.first_child[t];
        let i = base + k;

        self.modifier[Self::gap::<P>(i)] += dist;
        self.msel[i] += dist;
        self.mser[i] += dist;

        if si + 1 != k {
            // Are there intermediate children?
            let nr = (k - si) as f64;
            let ratio = dist / nr;
            self.shift[base + si + 1] += ratio;
            self.shift[i] -= ratio;
            self.change[i] -= dist - ratio;
        }
    }

    fn set_left_thread<const P: bool>(&mut self, base: usize, k: usize, cl: usize, modsumcl: f64) {
        let i = base + k;
        // An extreme node is a leaf, so it is the lower band that carries the thread.
        let le = self.el[base];
        let li = Self::con::<P>(le);

        self.tl[le] = cl;

        let diff = (modsumcl - self.modifier[cl]) - self.msel[base];
        self.modifier[li] += diff;
        self.prelim[li] -= diff;

        self.el[base] = self.el[i];
        self.msel[base] = self.msel[i];
    }

    fn set_right_thread<const P: bool>(&mut self, base: usize, k: usize, sr: usize, modsumsr: f64) {
        let i = base + k;
        let re = self.er[i];
        let ri = Self::con::<P>(re);

        self.tr[re] = sr;

        let diff = (modsumsr - self.modifier[sr]) - self.mser[i];
        self.modifier[ri] += diff;
        self.prelim[ri] -= diff;

        self.er[i] = self.er[i - 1];
        self.mser[i] = self.mser[i - 1];
    }

    /// `second_walk`: a forward sweep, level by level, returning the smallest breadth.
    ///
    /// Within a level the modifier sum is a gather from the parents, which stays scalar;
    /// the coordinate that follows from it is elementwise, and so is the minimum.
    fn second_sweep(&mut self, centeredxy: bool, k: Kernels) -> f64 {
        if self.paired {
            self.second_sweep_paired(centeredxy)
        } else {
            self.second_sweep_plain(centeredxy, k)
        }
    }

    /// [`Flat::second_sweep`], over a mirror of one band per entry.
    fn second_sweep_plain(&mut self, centeredxy: bool, k: Kernels) -> f64 {
        self.modsum[0] = self.modifier[0];

        let mut best = f64::INFINITY;

        for l in 0..self.levels.len() - 1 {
            let level = self.levels[l]..self.levels[l + 1];

            if l > 0 {
                for i in level.clone() {
                    self.modsum[i] = self.modsum[self.parent[i]] + self.modifier[i];
                }
            }

            k.add(
                &mut self.breadth[level.clone()],
                &self.prelim[level.clone()],
                &self.modsum[level.clone()],
            );

            if centeredxy {
                k.add_half(&mut self.breadth[level.clone()], &self.bext[level.clone()]);
                k.add_half(&mut self.depth[level.clone()], &self.dext[level.clone()]);
            }

            best = best.min(k.min(&self.breadth[level.clone()]));

            // `add_child_spacing`, the second-order prefix scan that has to stay scalar:
            // any parallel scan reassociates the additions and changes the last ULP. It
            // writes into the *next* level's modifiers, which is why it runs last.
            for i in level {
                let c = self.children(i);
                let mut d = 0.0;
                let mut modsumdelta = 0.0;
                for j in c {
                    d += self.shift[j];
                    modsumdelta += d + self.change[j];
                    self.modifier[j] += modsumdelta;
                }
            }
        }

        best
    }

    /// [`Flat::second_sweep`], over the two bands of every entry.
    ///
    /// The modifier sum runs through both bands, and both take part in the minimum -- a
    /// gap node contributes its own breadth to it in the recursion, even though nothing
    /// ever draws it. The elementwise kernels sit this one out: within a level the bands
    /// are strided, and a gather is not what they are for.
    fn second_sweep_paired(&mut self, centeredxy: bool) -> f64 {
        let mut best = f64::INFINITY;

        for l in 0..self.levels.len() - 1 {
            let level = self.levels[l]..self.levels[l + 1];

            for v in level.clone() {
                let (g, t) = (2 * v, 2 * v + 1);

                let up = if v == 0 {
                    0.0
                } else {
                    self.modsum[self.parent[v]]
                };

                let msg = up + self.modifier[g];
                let msc = msg + self.modifier[t];
                self.modsum[v] = msc;

                let mut bg = self.prelim[g] + msg;
                let mut b = self.prelim[t] + msc;

                if centeredxy {
                    bg += self.bext[g] / 2.0;
                    b += self.bext[t] / 2.0;
                    self.depth[v] += self.dext[t] / 2.0;
                }

                self.breadth[v] = b;
                best = best.min(bg).min(b);
            }

            // `add_child_spacing`, into the gap band of every child; see the plain sweep.
            for v in level {
                let c = self.children(v);
                let mut d = 0.0;
                let mut modsumdelta = 0.0;

                for j in c {
                    d += self.shift[j];
                    modsumdelta += d + self.change[j];
                    self.modifier[2 * j] += modsumdelta;
                }
            }
        }

        best
    }

    /// `third_walk`: one flat pass over everything, which is as elementwise as it gets.
    fn third_sweep(&mut self, dbreadth: f64, ddepth: f64, k: Kernels) {
        k.sub_scalar(&mut self.breadth, dbreadth);
        k.sub_scalar(&mut self.depth, ddepth);
    }

    fn write_back(&self, arena: &mut Arena, vertically: bool, centeredxy: bool) {
        for l in 0..self.levels.len() - 1 {
            for i in self.levels[l]..self.levels[l + 1] {
                let node = &mut arena[self.id[i]];

                if vertically {
                    node.x = self.breadth[i];
                    node.y = self.depth[i];
                } else {
                    node.x = self.depth[i];
                    node.y = self.breadth[i];
                }

                node.level = l;
                node.centeredxy = centeredxy;

                // The recursive path fills these in for children only, leaving whatever
                // the root already carried; do the same rather than clearing it.
                if self.parent[i] != NONE {
                    node.parent = Some(self.id[self.parent[i]]);
                    node.childno = self.childno[i];
                }
            }
        }
    }
}

/// A link of the chain of left siblings and their lowest depth coordinate.
#[derive(Debug, Clone, Copy)]
struct ChainLink {
    low: f64,
    index: usize,
}

fn update_chain(chain: &mut Vec<ChainLink>, min: f64, i: usize) {
    while let Some(top) = chain.last() {
        if min >= top.low {
            chain.pop();
        } else {
            break;
        }
    }
    chain.push(ChainLink { low: min, index: i });
}

/// The elementwise sweeps, scalar or vectorized.
///
/// Every kernel here is elementwise or a `min` reduction, which is why widening them
/// cannot change a result: elementwise operations do not reassociate, and `min` is exact
/// under any association. The prefix scan of `add_child_spacing` is deliberately not here.
#[derive(Debug, Clone, Copy, Default)]
pub struct Kernels {
    simd: bool,
}

impl Kernels {
    /// Plain loops, which the compiler is free to vectorize on its own.
    pub fn scalar() -> Kernels {
        Kernels { simd: false }
    }

    /// AVX2 where the CPU has it, plain loops otherwise.
    #[cfg(feature = "simd")]
    pub fn detect() -> Kernels {
        #[cfg(target_arch = "x86_64")]
        let simd = simd::available();
        #[cfg(not(target_arch = "x86_64"))]
        let simd = false;

        Kernels { simd }
    }

    /// Whether the vectorized kernels are the ones in use.
    pub fn is_simd(self) -> bool {
        self.simd
    }

    /// `dst[i] = v`
    fn fill(self, dst: &mut [f64], v: f64) {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            unsafe { simd::fill(dst, v) };
            return;
        }
        for d in dst {
            *d = v;
        }
    }

    /// `dst[i] = v + src[i]`
    fn add_scalar(self, dst: &mut [f64], src: &[f64], v: f64) {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            unsafe { simd::add_scalar(dst, src, v) };
            return;
        }
        for (d, s) in dst.iter_mut().zip(src) {
            *d = v + *s;
        }
    }

    /// `dst[i] = a[i] + b[i]`
    fn add(self, dst: &mut [f64], a: &[f64], b: &[f64]) {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            unsafe { simd::add(dst, a, b) };
            return;
        }
        for (d, (x, y)) in dst.iter_mut().zip(a.iter().zip(b)) {
            *d = *x + *y;
        }
    }

    /// `dst[i] += src[i] / 2`
    fn add_half(self, dst: &mut [f64], src: &[f64]) {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            unsafe { simd::add_half(dst, src) };
            return;
        }
        for (d, s) in dst.iter_mut().zip(src) {
            *d += *s / 2.0;
        }
    }

    /// `dst[i] -= v`
    fn sub_scalar(self, dst: &mut [f64], v: f64) {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            unsafe { simd::sub_scalar(dst, v) };
            return;
        }
        for d in dst {
            *d -= v;
        }
    }

    /// The smallest element, or `+inf` for an empty slice.
    fn min(self, v: &[f64]) -> f64 {
        #[cfg(all(feature = "simd", target_arch = "x86_64"))]
        if self.simd {
            // SAFETY: `simd` is only set when the CPU reports AVX2.
            return unsafe { simd::min(v) };
        }
        v.iter()
            .fold(f64::INFINITY, |m, &c| if c < m { c } else { m })
    }
}

/// AVX2 kernels: four doubles at a time, with a scalar tail.
///
/// Every one of these is an elementwise operation or a `min` reduction, so the result is
/// bit-identical to the scalar loop above it whatever the lane count.
#[cfg(all(feature = "simd", target_arch = "x86_64"))]
mod simd {
    use std::arch::x86_64::*;

    pub fn available() -> bool {
        is_x86_feature_detected!("avx2")
    }

    const LANES: usize = 4;

    #[target_feature(enable = "avx2")]
    pub unsafe fn fill(dst: &mut [f64], v: f64) {
        let w = _mm256_set1_pd(v);
        let chunks = dst.len() / LANES;
        for c in 0..chunks {
            _mm256_storeu_pd(dst.as_mut_ptr().add(c * LANES), w);
        }
        for d in &mut dst[chunks * LANES..] {
            *d = v;
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn add_scalar(dst: &mut [f64], src: &[f64], v: f64) {
        let n = dst.len().min(src.len());
        let w = _mm256_set1_pd(v);
        let chunks = n / LANES;
        for c in 0..chunks {
            let s = _mm256_loadu_pd(src.as_ptr().add(c * LANES));
            _mm256_storeu_pd(dst.as_mut_ptr().add(c * LANES), _mm256_add_pd(w, s));
        }
        for i in chunks * LANES..n {
            *dst.get_unchecked_mut(i) = v + *src.get_unchecked(i);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn add(dst: &mut [f64], a: &[f64], b: &[f64]) {
        let n = dst.len().min(a.len()).min(b.len());
        let chunks = n / LANES;
        for c in 0..chunks {
            let x = _mm256_loadu_pd(a.as_ptr().add(c * LANES));
            let y = _mm256_loadu_pd(b.as_ptr().add(c * LANES));
            _mm256_storeu_pd(dst.as_mut_ptr().add(c * LANES), _mm256_add_pd(x, y));
        }
        for i in chunks * LANES..n {
            *dst.get_unchecked_mut(i) = *a.get_unchecked(i) + *b.get_unchecked(i);
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn add_half(dst: &mut [f64], src: &[f64]) {
        let n = dst.len().min(src.len());
        let half = _mm256_set1_pd(0.5);
        let chunks = n / LANES;
        for c in 0..chunks {
            let d = _mm256_loadu_pd(dst.as_ptr().add(c * LANES));
            let s = _mm256_loadu_pd(src.as_ptr().add(c * LANES));
            // `* 0.5` and `/ 2.0` are the same operation on a binary float: an exact
            // exponent decrement, with no rounding to disagree about.
            _mm256_storeu_pd(
                dst.as_mut_ptr().add(c * LANES),
                _mm256_add_pd(d, _mm256_mul_pd(s, half)),
            );
        }
        for i in chunks * LANES..n {
            *dst.get_unchecked_mut(i) += *src.get_unchecked(i) / 2.0;
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn sub_scalar(dst: &mut [f64], v: f64) {
        let w = _mm256_set1_pd(v);
        let chunks = dst.len() / LANES;
        for c in 0..chunks {
            let d = _mm256_loadu_pd(dst.as_ptr().add(c * LANES));
            _mm256_storeu_pd(dst.as_mut_ptr().add(c * LANES), _mm256_sub_pd(d, w));
        }
        for d in &mut dst[chunks * LANES..] {
            *d -= v;
        }
    }

    #[target_feature(enable = "avx2")]
    pub unsafe fn min(v: &[f64]) -> f64 {
        let chunks = v.len() / LANES;

        let mut acc = _mm256_set1_pd(f64::INFINITY);
        for c in 0..chunks {
            acc = _mm256_min_pd(acc, _mm256_loadu_pd(v.as_ptr().add(c * LANES)));
        }

        let mut lanes = [0.0f64; LANES];
        _mm256_storeu_pd(lanes.as_mut_ptr(), acc);

        let mut m = f64::INFINITY;
        for &c in lanes.iter().chain(&v[chunks * LANES..]) {
            if c < m {
                m = c;
            }
        }
        m
    }
}

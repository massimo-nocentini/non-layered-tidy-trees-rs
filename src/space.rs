//! Non-layered tidy trees in three dimensions.
//!
//! Every node is a box `w` × `h` × `d`: `w` along `x`, `h` along the depth axis `y` (top
//! down, exactly as in [`crate::layout`]), and `d` along `z`. The depth axis stays
//! non-layered — a child starts at its parent's bottom — and the two breadth axes `x` and
//! `z` form the plane the children of a node are spread over.
//!
//! The 2D algorithm is linear because merging a subtree into its left siblings is one
//! splice of two threaded contours. That needs the siblings to be ordered along a line,
//! so here every internal node `v` picks a *family axis* `a(v)`, `x` or `z`, and
//!
//! * lays its children out along `a(v)` with the very same `separate`, sibling chain,
//!   `move_subtree` and `position_root` as [`crate::layout`], on the contours along `a(v)`;
//! * centres each child's box under its own box along the other axis `b(v)`.
//!
//! Each subtree keeps four threaded contours, left and right along both `x` and `z`.
//! Along `a(v)` they are the 2D splice. Along `b(v)` the children sit side by side in
//! depth rather than in breadth, so the contour of `v` is `v` itself, then one [`Item::Hull`]
//! covering the depths down to the second deepest child with the extreme of everything
//! there, then a thread into the deepest child's own contour. Either way a merge is a
//! prefix plus one thread, every item stepped past is hidden for good, and the whole
//! layout stays linear in the number of nodes.
//!
//! The guarantee is the 2D one, per family: take two nodes whose depth ranges overlap,
//! neither an ancestor of the other, and their lowest common ancestor `u`; along `a(u)`
//! the earlier one's far face plus its margin does not pass the later one's near face, so
//! the two boxes are disjoint. Along `a(u)` the children keep their order, the parent is
//! centred over its first and last child and the middle siblings are spread evenly, all
//! as in 2D. The price is compactness: siblings are always separated along their family's
//! axis, even where they are already apart along the other one, and a hull keeps a
//! subtree's full extent at every depth it covers.
//!
//! With every family along `x` ([`AxisRule::AllX`]), `x` and `y` come out bit for bit as
//! [`crate::layout`] draws the same tree, whatever the `d` of each box.
//!
//! ```
//! use non_layered_tidy_trees::space::{layout3, Arena3, AxisRule, LayoutInput3};
//!
//! let mut arena = Arena3::new();
//! let root = arena.add_node(1, 10.0, 10.0, 10.0, 0.0); // idx, w, h, d, margin
//! let a = arena.add_node(2, 10.0, 10.0, 10.0, 0.0);
//! let b = arena.add_node(3, 10.0, 10.0, 10.0, 0.0);
//! let c = arena.add_node(4, 10.0, 10.0, 10.0, 0.0);
//! let e = arena.add_node(5, 10.0, 10.0, 10.0, 0.0);
//! arena.set_children(root, &[a, b]);
//! arena.set_children(a, &[c, e]);
//!
//! // the parents of leaves spread along x, their parents along z, and so on upwards
//! layout3(&mut arena, &LayoutInput3 { rule: AxisRule::Alternate, ..LayoutInput3::new(root) });
//!
//! assert_eq!((arena[c].x, arena[e].x), (0.0, 10.0)); // a's children, along x
//! assert_eq!((arena[a].z, arena[b].z), (0.0, 10.0)); // the root's children, along z
//! assert_eq!((arena[b].x, arena[b].y), (5.0, 10.0)); // b centred under the root along x
//! ```

use std::ops::{Index, IndexMut};

use crate::{update_chain, Arena, ChainLink, Node, NodeId};

/// One of the two breadth axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Breadth {
    /// Along `x`, where a box measures `w`.
    #[default]
    X,
    /// Along `z`, where a box measures `d`.
    Z,
}

impl Breadth {
    /// The other breadth axis.
    pub fn other(self) -> Breadth {
        match self {
            Breadth::X => Breadth::Z,
            Breadth::Z => Breadth::X,
        }
    }

    fn ix(self) -> usize {
        match self {
            Breadth::X => 0,
            Breadth::Z => 1,
        }
    }

    fn of(ax: usize) -> Breadth {
        if ax == 0 {
            Breadth::X
        } else {
            Breadth::Z
        }
    }
}

/// Which axis each family is laid out along, for the nodes that do not say.
///
/// [`Node3::axis`] overrides the rule node by node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AxisRule {
    /// Every family along `x`: the 2D drawing, with every child centred along `z`.
    #[default]
    AllX,
    /// Every family along `z`.
    AllZ,
    /// By the height of the subtree: odd heights along `x`, even heights along `z`.
    ///
    /// The parents of leaves only — height 1 — spread along `x`, their parents along `z`,
    /// and so on upwards. The height depends on the subtree alone, so isomorphic subtrees
    /// get the same axes and come out congruent; a rule by level would not.
    Alternate,
}

impl AxisRule {
    fn pick(self, height: usize) -> Breadth {
        match self {
            AxisRule::AllX => Breadth::X,
            AxisRule::AllZ => Breadth::Z,
            AxisRule::Alternate => {
                if height % 2 == 1 {
                    Breadth::X
                } else {
                    Breadth::Z
                }
            }
        }
    }
}

/// Which side of a contour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The side facing the smaller coordinates.
    Left,
    /// The side facing the larger coordinates.
    Right,
}

impl Side {
    fn ix(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// A step of a contour, as [`Callbacks3::contour_pairs`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// A node's own box.
    Node(NodeId),
    /// The hull of a node's children along the node's cross axis, down to the bottom of the
    /// second deepest child: the extreme of their boxes on that side.
    Hull(NodeId, Side),
}

/// The state of an [`Item::Hull`].
///
/// A hull lives on one side of one axis, so a single thread is all it needs.
#[derive(Debug, Clone, Copy, Default)]
struct Hull {
    bottom: f64,
    prelim: f64,
    modifier: f64,
    thread: Option<Item>,
}

/// A node of a tree laid out in three dimensions; the counterpart of [`Node`].
///
/// `w`, `h`, `d`, `margin` and `axis` are the input; `x`, `y` and `z` are the output.
#[derive(Debug, Clone)]
pub struct Node3 {
    /// A caller-chosen label.
    pub idx: usize,
    /// Distance from the root, set by [`layout3`]; the root is at level 0.
    pub level: usize,
    /// Position among the parent's children, set by [`layout3`]; 0 for the root.
    pub childno: usize,
    /// Whether `x`/`y`/`z` denote the middle of the box rather than its smallest corner.
    ///
    /// Set by [`layout3`] from [`LayoutInput3::centered`]; `false` until then.
    pub centered: bool,
    /// Extent along `x`.
    pub w: f64,
    /// Extent along the depth axis `y`.
    pub h: f64,
    /// Extent along `z`.
    pub d: f64,
    /// Extra separation kept on the far side of the box along both breadth axes: from the
    /// sibling subtree that follows along the family axis, and as part of the hull along
    /// the other.
    pub margin: f64,
    /// The axis this node's children are laid out along; `None` leaves it to
    /// [`LayoutInput3::rule`].
    pub axis: Option<Breadth>,
    /// Breadth coordinate.
    pub x: f64,
    /// Depth coordinate.
    pub y: f64,
    /// Breadth coordinate.
    pub z: f64,
    /// The parent, set by [`layout3`]; `None` for the root.
    pub parent: Option<NodeId>,

    children: Vec<NodeId>,
    /// The resolved family axis.
    fam: Breadth,

    // Everything per breadth axis is indexed by `Breadth::ix`.
    prelim: [f64; 2],
    modifier: [f64; 2],
    /// Along the family axis of the parent, the only one siblings are spread along.
    shift: f64,
    change: f64,
    tl: [Option<Item>; 2],
    tr: [Option<Item>; 2],
    el: [Option<Item>; 2],
    er: [Option<Item>; 2],
    msel: [f64; 2],
    mser: [f64; 2],

    /// The deepest bottom of the subtree.
    maxdepth: f64,
    /// The subtree's extent as `[xmin, xmax, zmin, zmax]`, relative to this node's own
    /// corner and with the margins on the far faces.
    ///
    /// Relative to the corner and not to `prelim`: the thread correction moves a leaf's
    /// `prelim` and `modifier` in opposite directions, and only their sum is stable.
    bbox: [f64; 4],
    /// Left and right hull along the cross axis, for a node of two children or more.
    hulls: [Hull; 2],
}

impl Node3 {
    fn new(idx: usize, w: f64, h: f64, d: f64, margin: f64) -> Self {
        Node3 {
            idx,
            level: 0,
            childno: 0,
            centered: false,
            w,
            h,
            d,
            margin,
            axis: None,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            parent: None,
            children: Vec::new(),
            fam: Breadth::X,
            prelim: [0.0; 2],
            modifier: [0.0; 2],
            shift: 0.0,
            change: 0.0,
            tl: [None; 2],
            tr: [None; 2],
            el: [None; 2],
            er: [None; 2],
            msel: [0.0; 2],
            mser: [0.0; 2],
            maxdepth: 0.0,
            bbox: [0.0; 4],
            hulls: [Hull::default(); 2],
        }
    }

    /// The children, in order along the family axis.
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    /// The axis the children were laid out along, as [`layout3`] resolved it.
    pub fn family(&self) -> Breadth {
        self.fam
    }

    /// The extent along a breadth axis.
    pub fn extent(&self, ax: Breadth) -> f64 {
        self.ext(ax.ix())
    }

    fn ext(&self, ax: usize) -> f64 {
        if ax == 0 {
            self.w
        } else {
            self.d
        }
    }
}

/// The nodes of one or more trees, the counterpart of [`Arena`].
#[derive(Debug, Clone, Default)]
pub struct Arena3 {
    nodes: Vec<Node3>,
}

impl Index<NodeId> for Arena3 {
    type Output = Node3;

    fn index(&self, id: NodeId) -> &Node3 {
        &self.nodes[id.0]
    }
}

impl IndexMut<NodeId> for Arena3 {
    fn index_mut(&mut self, id: NodeId) -> &mut Node3 {
        &mut self.nodes[id.0]
    }
}

impl Arena3 {
    /// An empty arena.
    pub fn new() -> Self {
        Arena3 { nodes: Vec::new() }
    }

    /// An empty arena with room for `capacity` nodes.
    pub fn with_capacity(capacity: usize) -> Self {
        Arena3 {
            nodes: Vec::with_capacity(capacity),
        }
    }

    /// Adds a childless node.
    pub fn add_node(&mut self, idx: usize, w: f64, h: f64, d: f64, margin: f64) -> NodeId {
        self.nodes.push(Node3::new(idx, w, h, d, margin));
        NodeId(self.nodes.len() - 1)
    }

    /// Appends one child to `parent`.
    pub fn push_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[parent.0].children.push(child);
    }

    /// Replaces the children of `parent`, in order.
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) {
        self.nodes[parent.0].children.clear();
        self.nodes[parent.0].children.extend_from_slice(children);
    }

    /// The children of `t`, in order.
    pub fn children(&self, t: NodeId) -> &[NodeId] {
        &self.nodes[t.0].children
    }

    /// How many nodes the arena holds.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the arena holds no node at all.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The nodes, in creation order.
    pub fn iter(&self) -> impl Iterator<Item = &Node3> {
        self.nodes.iter()
    }

    /// The ids of the subtree rooted at `t`, in preorder.
    pub fn preorder(&self, t: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![t];
        while let Some(v) = stack.pop() {
            out.push(v);
            stack.extend(self.nodes[v.0].children.iter().rev());
        }
        out
    }

    /// The bottom of a node's box along the depth axis.
    pub fn bottom(&self, t: NodeId) -> f64 {
        let n = &self.nodes[t.0];
        n.y + if n.centered { n.h / 2.0 } else { n.h }
    }

    /// A copy of the tree rooted at `root` in a 2D arena, with `depth` giving each box its
    /// `d`; the ids follow the preorder, the labels and margins are kept.
    pub fn lift(
        arena: &Arena,
        root: NodeId,
        mut depth: impl FnMut(&Node) -> f64,
    ) -> (Arena3, NodeId) {
        let mut out = Arena3::with_capacity(arena.len());
        let mut stack = vec![(root, None)];

        while let Some((v, parent)) = stack.pop() {
            let n = &arena[v];
            let id = out.add_node(n.idx, n.w, n.h, depth(n), n.margin);
            if let Some(p) = parent {
                out.push_child(p, id);
            }
            stack.extend(arena.children(v).iter().rev().map(|&c| (c, Some(id))));
        }

        (out, NodeId(0))
    }

    fn child(&self, t: NodeId, i: usize) -> NodeId {
        self.nodes[t.0].children[i]
    }

    fn nchildren(&self, t: NodeId) -> usize {
        self.nodes[t.0].children.len()
    }

    fn hull(&self, t: NodeId, side: Side) -> &Hull {
        &self.nodes[t.0].hulls[side.ix()]
    }

    fn hull_mut(&mut self, t: NodeId, side: Side) -> &mut Hull {
        &mut self.nodes[t.0].hulls[side.ix()]
    }

    fn item_bottom(&self, it: Item) -> f64 {
        match it {
            Item::Node(n) => self.bottom(n),
            Item::Hull(n, s) => self.hull(n, s).bottom,
        }
    }

    fn item_modifier(&self, it: Item, ax: usize) -> f64 {
        match it {
            Item::Node(n) => self[n].modifier[ax],
            Item::Hull(n, s) => self.hull(n, s).modifier,
        }
    }

    /// Where the item starts along `ax`, given the modifiers summed down to it.
    fn near(&self, sum: f64, it: Item, ax: usize) -> f64 {
        match it {
            Item::Node(n) => sum + self[n].prelim[ax],
            Item::Hull(n, s) => sum + self.hull(n, s).prelim,
        }
    }

    /// Where the item ends along `ax`, margin included; a hull holds its far side.
    fn far(&self, sum: f64, it: Item, ax: usize) -> f64 {
        match it {
            Item::Node(n) => {
                let n = &self[n];
                sum + n.prelim[ax] + n.ext(ax) + n.margin
            }
            Item::Hull(n, s) => sum + self.hull(n, s).prelim,
        }
    }

    /// Moves the modifier of an item by `diff` without moving the item.
    fn nudge(&mut self, it: Item, ax: usize, diff: f64) {
        match it {
            Item::Node(n) => {
                let n = &mut self[n];
                n.modifier[ax] += diff;
                n.prelim[ax] -= diff;
            }
            Item::Hull(n, s) => {
                let h = self.hull_mut(n, s);
                h.modifier += diff;
                h.prelim -= diff;
            }
        }
    }

    fn set_thread(&mut self, it: Item, side: Side, ax: usize, to: Item) {
        match it {
            Item::Node(n) => match side {
                Side::Left => self[n].tl[ax] = Some(to),
                Side::Right => self[n].tr[ax] = Some(to),
            },
            Item::Hull(n, s) => {
                debug_assert_eq!(s, side, "a hull only ever ends a contour of its own side");
                self.hull_mut(n, s).thread = Some(to);
            }
        }
    }

    /// The item below `it` on the `side` contour along `ax`.
    fn next(&self, it: Item, side: Side, ax: usize) -> Option<Item> {
        match it {
            Item::Node(t) => {
                let n = &self[t];
                match n.children.len() {
                    0 => match side {
                        Side::Left => n.tl[ax],
                        Side::Right => n.tr[ax],
                    },
                    _ if n.fam.ix() == ax => match side {
                        Side::Left => n.children.first().map(|&c| Item::Node(c)),
                        Side::Right => n.children.last().map(|&c| Item::Node(c)),
                    },
                    1 => Some(Item::Node(n.children[0])),
                    _ => Some(Item::Hull(t, side)),
                }
            }
            Item::Hull(n, s) => self.hull(n, s).thread,
        }
    }
}

/// What to lay out, and how; the counterpart of [`crate::LayoutInput`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutInput3 {
    /// The root of the tree to lay out.
    pub root: NodeId,
    /// Whether `x`/`y`/`z` should denote the middle of each box rather than its corner.
    pub centered: bool,
    /// The family axis of the nodes whose [`Node3::axis`] is `None`.
    pub rule: AxisRule,
    /// Where the normalized drawing starts along `x`.
    pub x: f64,
    /// Where the root starts along `y`.
    pub y: f64,
    /// Where the normalized drawing starts along `z`.
    pub z: f64,
}

impl LayoutInput3 {
    /// A drawing of `root` with every family along `x`, in corner coordinates, starting at
    /// the origin.
    pub fn new(root: NodeId) -> Self {
        LayoutInput3 {
            root,
            centered: false,
            rule: AxisRule::AllX,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

/// Called once per node as the second walk finalizes it.
pub type WalkFn3<'a> = dyn FnMut(&mut Arena3, NodeId) + 'a;

/// Called for every contour pair `separate` compares, with the axis and the `dist`.
pub type ContourPairsFn3<'a> = dyn FnMut(&Arena3, Item, Item, Breadth, f64) + 'a;

/// The two hooks of [`layout3_with`], the counterpart of [`crate::Callbacks`].
#[derive(Default)]
pub struct Callbacks3<'a> {
    /// Called once per node, in preorder, during the second walk.
    pub walk: Option<&'a mut WalkFn3<'a>>,
    /// Called for every contour pair `(sr, cl, axis, dist)` compared while separating
    /// siblings.
    pub contour_pairs: Option<&'a mut ContourPairsFn3<'a>>,
}

/// Process change and shift to add intermediate spacing to mod, along the family axis.
fn add_child_spacing(a: &mut Arena3, t: NodeId) {
    let ax = a[t].fam.ix();
    let mut d = 0.0;
    let mut modsumdelta = 0.0;
    for i in 0..a.nchildren(t) {
        let ci = a.child(t, i);
        let c = &mut a[ci];
        d += c.shift;
        modsumdelta += d + c.change;
        c.modifier[ax] += modsumdelta;
    }
}

fn move_subtree(a: &mut Arena3, t: NodeId, i: usize, si: usize, dist: f64, ax: usize) {
    let ci = a.child(t, i);
    let c = &mut a[ci];
    c.modifier[ax] += dist;
    c.msel[ax] += dist;
    c.mser[ax] += dist;

    if si + 1 != i {
        // Are there intermediate children?
        let nr = (i - si) as f64;
        let ratio = dist / nr;
        let cs = a.child(t, si + 1);
        a[cs].shift += ratio;
        let c = &mut a[ci];
        c.shift -= ratio;
        c.change -= dist - ratio;
    }
}

fn set_left_thread(a: &mut Arena3, t: NodeId, i: usize, cl: Item, modsumcl: f64, ax: usize) {
    let c0 = a.child(t, 0);
    let ci = a.child(t, i);
    let li = a[c0].el[ax].expect("the leftmost child has an extreme left item");

    // Change mod so that the sum of modifier after following thread is correct.
    let diff = (modsumcl - a.item_modifier(cl, ax)) - a[c0].msel[ax];

    a.set_thread(li, Side::Left, ax, cl);
    a.nudge(li, ax, diff);

    // Update extreme item and its sum of modifiers.
    a[c0].el[ax] = a[ci].el[ax];
    a[c0].msel[ax] = a[ci].msel[ax];
}

/// Symmetrical to [`set_left_thread`].
fn set_right_thread(a: &mut Arena3, t: NodeId, i: usize, sr: Item, modsumsr: f64, ax: usize) {
    let ci = a.child(t, i);
    let cp = a.child(t, i - 1);
    let ri = a[ci].er[ax].expect("the i-th child has an extreme right item");
    let diff = (modsumsr - a.item_modifier(sr, ax)) - a[ci].mser[ax];

    a.set_thread(ri, Side::Right, ax, sr);
    a.nudge(ri, ax, diff);

    a[ci].er[ax] = a[cp].er[ax];
    a[ci].mser[ax] = a[cp].mser[ax];
}

/// [`crate::layout`]'s `separate`, along `ax` and over items rather than nodes.
fn separate(
    a: &mut Arena3,
    cb: &mut Callbacks3,
    t: NodeId,
    i: usize,
    chain: &[ChainLink],
    ax: usize,
) {
    let prev = a.child(t, i - 1);
    let mut sr = Some(Item::Node(prev));
    let mut mssr = a[prev].modifier[ax];

    let cur = a.child(t, i);
    let mut cl = Some(Item::Node(cur));
    let mut mscl = a[cur].modifier[ax];

    let mut ih = chain.len() - 1;

    let mut first = true;

    while let (Some(s), Some(c)) = (sr, cl) {
        let sy = a.item_bottom(s);
        let cy = a.item_bottom(c);

        if sy > chain[ih].low {
            ih = ih.checked_sub(1).expect(
                "the sibling chain is seeded with a value that dominates the subtree depth",
            );
        }

        // How far to the left of the right side of sr is the left side of cl?
        let dist = a.far(mssr, s, ax) - a.near(mscl, c, ax);

        // Pulling the subtree closer is only sound for the topmost pair, as in 2D.
        if (first && dist < 0.0) || dist > 0.0 {
            mscl += dist;
            move_subtree(a, t, i, chain[ih].index, dist, ax);
        }

        first = false;

        if let Some(f) = cb.contour_pairs.as_mut() {
            f(a, s, c, Breadth::of(ax), dist);
        }

        if sy <= cy {
            sr = a.next(s, Side::Right, ax);
            if let Some(n) = sr {
                mssr += a.item_modifier(n, ax);
            }
        }

        if sy >= cy {
            cl = a.next(c, Side::Left, ax);
            if let Some(n) = cl {
                mscl += a.item_modifier(n, ax);
            }
        }
    }

    match (sr, cl) {
        (None, Some(c)) => set_left_thread(a, t, i, c, mscl, ax),
        (Some(s), None) => set_right_thread(a, t, i, s, mssr, ax),
        _ => {}
    }
}

fn position_root(a: &mut Arena3, t: NodeId, ax: usize) {
    // Position root between children, taking into account their mod.
    let first = a.child(t, 0);
    let last = a.child(t, a.nchildren(t) - 1);
    let (f, l, n) = (&a[first], &a[last], &a[t]);

    let d = l.ext(ax) - n.ext(ax);

    a[t].prelim[ax] = (f.prelim[ax] + f.modifier[ax] + l.prelim[ax] + l.modifier[ax] + d) / 2.0;
}

/// Builds the `side` contour of `t` along its cross axis `b`, for two children or more:
/// `t`, then a hull down to the second deepest child, then a thread into the deepest.
fn hull_contour(a: &mut Arena3, t: NodeId, b: usize, side: Side, j1: usize, d2: f64) {
    let extreme = |x: f64, y: f64| match side {
        Side::Left => {
            if y < x {
                y
            } else {
                x
            }
        }
        Side::Right => {
            if y > x {
                y
            } else {
                x
            }
        }
    };

    // The deepest child's own contour, down to and including the first item that reaches
    // below the second deepest child: that one straddles the hull's bottom.
    let deep = a.child(t, j1);
    let mut it = Item::Node(deep);
    let mut s = a[deep].modifier[b];
    let mut ext = match side {
        Side::Left => f64::INFINITY,
        Side::Right => f64::NEG_INFINITY,
    };
    let tee = loop {
        let e = match side {
            Side::Left => a.near(s, it, b),
            Side::Right => a.far(s, it, b),
        };
        ext = extreme(ext, e);
        if a.item_bottom(it) > d2 {
            break Some((it, s));
        }
        match a.next(it, side, b) {
            Some(n) => {
                s += a.item_modifier(n, b);
                it = n;
            }
            None => break None,
        }
    };

    // Every other child is no deeper than the hull, so its whole box counts.
    for j in 0..a.nchildren(t) {
        if j == j1 {
            continue;
        }
        let c = &a[a.child(t, j)];
        let rel = match side {
            Side::Left => c.bbox[2 * b],
            Side::Right => c.bbox[2 * b + 1],
        };
        ext = extreme(ext, c.prelim[b] + c.modifier[b] + rel);
    }

    let k = side.ix();
    let hull = match tee {
        Some((tee, st)) => {
            // The sums telescope: through the hull, `tee` is reached with the same sum as
            // through the deepest child's own contour.
            let m = st - a.item_modifier(tee, b);
            let (el, ms) = match side {
                Side::Left => (a[deep].el[b], a[deep].msel[b]),
                Side::Right => (a[deep].er[b], a[deep].mser[b]),
            };
            set_extreme(a, t, side, b, el, ms);
            Hull {
                bottom: d2,
                prelim: ext - m,
                modifier: m,
                thread: Some(tee),
            }
        }
        None => {
            set_extreme(a, t, side, b, Some(Item::Hull(t, side)), 0.0);
            Hull {
                bottom: d2,
                prelim: ext,
                modifier: 0.0,
                thread: None,
            }
        }
    };
    a[t].hulls[k] = hull;
}

fn set_extreme(a: &mut Arena3, t: NodeId, side: Side, ax: usize, it: Option<Item>, ms: f64) {
    let n = &mut a[t];
    match side {
        Side::Left => {
            n.el[ax] = it;
            n.msel[ax] = ms;
        }
        Side::Right => {
            n.er[ax] = it;
            n.mser[ax] = ms;
        }
    }
}

fn first_walk(a: &mut Arena3, cb: &mut Callbacks3, t: NodeId) {
    let k = a.nchildren(t);

    if k == 0 {
        // setting extremes, along both axes
        let n = &mut a[t];
        n.el = [Some(Item::Node(t)); 2];
        n.er = [Some(Item::Node(t)); 2];
        n.msel = [0.0; 2];
        n.mser = [0.0; 2];
        n.bbox = [0.0, n.w + n.margin, 0.0, n.d + n.margin];
        n.maxdepth = n.y + n.h;
        return;
    }

    let ax = a[t].fam.ix();
    let b = 1 - ax;

    // Along the family axis: the 2D first walk, verbatim.
    let c0 = a.child(t, 0);
    first_walk(a, cb, c0);

    let mut chain = Vec::new();
    let el = a[c0].el[ax].expect("the first walk sets the extreme left item");
    update_chain(&mut chain, a.item_bottom(el), 0);

    for i in 1..k {
        let ci = a.child(t, i);
        first_walk(a, cb, ci);

        let er = a[ci].er[ax].expect("the first walk sets the extreme right item");
        let min = a.item_bottom(er);

        separate(a, cb, t, i, &chain, ax);

        update_chain(&mut chain, min, i);
    }

    position_root(a, t, ax);

    let last = a.child(t, k - 1);
    let (el, msel) = (a[c0].el[ax], a[c0].msel[ax]);
    let (er, mser) = (a[last].er[ax], a[last].mser[ax]);
    set_extreme(a, t, Side::Left, ax, el, msel);
    set_extreme(a, t, Side::Right, ax, er, mser);

    // Along the cross axis: every child centred under the parent's box.
    for i in 0..k {
        let ci = a.child(t, i);
        let target = a[t].prelim[b] + (a[t].ext(b) - a[ci].ext(b)) / 2.0;
        let c = &mut a[ci];
        let off = target - (c.prelim[b] + c.modifier[b]);
        c.modifier[b] += off;
        c.msel[b] += off;
        c.mser[b] += off;
    }

    let mut d1 = f64::NEG_INFINITY;
    let mut j1 = 0;
    for i in 0..k {
        let m = a[a.child(t, i)].maxdepth;
        if m > d1 {
            d1 = m;
            j1 = i;
        }
    }

    if k == 1 {
        let (el, msel) = (a[c0].el[b], a[c0].msel[b]);
        let (er, mser) = (a[c0].er[b], a[c0].mser[b]);
        set_extreme(a, t, Side::Left, b, el, msel);
        set_extreme(a, t, Side::Right, b, er, mser);
    } else {
        let mut d2 = f64::NEG_INFINITY;
        for i in 0..k {
            let m = a[a.child(t, i)].maxdepth;
            if i != j1 && m > d2 {
                d2 = m;
            }
        }
        hull_contour(a, t, b, Side::Left, j1, d2);
        hull_contour(a, t, b, Side::Right, j1, d2);
    }

    // The subtree's extent, with the spacing the second walk will add to the middle
    // children replayed in the same order `add_child_spacing` applies it.
    let n = &a[t];
    let mut bbox = [0.0, n.w + n.margin, 0.0, n.d + n.margin];
    let origin = n.prelim;
    let (mut d, mut modsumdelta) = (0.0, 0.0);
    for i in 0..k {
        let c = &a[a.child(t, i)];
        d += c.shift;
        modsumdelta += d + c.change;
        let mut rel = [
            c.prelim[0] + c.modifier[0] - origin[0],
            c.prelim[1] + c.modifier[1] - origin[1],
        ];
        rel[ax] += modsumdelta;
        for q in 0..2 {
            let (lo, hi) = (rel[q] + c.bbox[2 * q], rel[q] + c.bbox[2 * q + 1]);
            if lo < bbox[2 * q] {
                bbox[2 * q] = lo;
            }
            if hi > bbox[2 * q + 1] {
                bbox[2 * q + 1] = hi;
            }
        }
    }

    let bottom = a.bottom(t);
    let n = &mut a[t];
    n.bbox = bbox;
    n.maxdepth = if d1 > bottom { d1 } else { bottom };
}

fn second_walk(
    a: &mut Arena3,
    input: &LayoutInput3,
    cb: &mut Callbacks3,
    t: NodeId,
    modsum_init: [f64; 2],
) -> [f64; 2] {
    let n = &mut a[t];

    let modsum = [
        modsum_init[0] + n.modifier[0],
        modsum_init[1] + n.modifier[1],
    ];
    let dx = n.prelim[0] + modsum[0];
    let dz = n.prelim[1] + modsum[1];

    let (xoffset, yoffset, zoffset) = if input.centered {
        (n.w / 2.0, n.h / 2.0, n.d / 2.0)
    } else {
        (0.0, 0.0, 0.0)
    };
    n.centered = input.centered;

    n.x = dx + xoffset;
    n.y += yoffset;
    n.z = dz + zoffset;
    let mut best = [n.x, n.z];

    add_child_spacing(a, t);

    if let Some(f) = cb.walk.as_mut() {
        f(a, t);
    }

    for i in 0..a.nchildren(t) {
        let c = a.child(t, i);
        let current = second_walk(a, input, cb, c, modsum);
        for q in 0..2 {
            if current[q] < best[q] {
                best[q] = current[q];
            }
        }
    }

    best
}

/// Resets what the walks accumulate into, sets the depth coordinates and resolves the
/// family axes; returns the height of the subtree.
fn setup_walk(a: &mut Arena3, input: &LayoutInput3, t: NodeId, level: usize) -> usize {
    let n = &mut a[t];

    n.level = level;
    n.centered = false;

    n.prelim = [0.0; 2];
    n.modifier = [0.0; 2];
    n.shift = 0.0;
    n.change = 0.0;
    n.msel = [0.0; 2];
    n.mser = [0.0; 2];
    n.tl = [None; 2];
    n.tr = [None; 2];
    n.el = [None; 2];
    n.er = [None; 2];
    n.maxdepth = 0.0;
    n.bbox = [0.0; 4];
    n.hulls = [Hull::default(); 2];

    if level == 0 {
        n.y = 0.0;
    }

    let b = a.bottom(t);
    let mut height = 0;

    for i in 0..a.nchildren(t) {
        let child = a.child(t, i);
        let c = &mut a[child];

        c.childno = i;
        c.parent = Some(t);
        c.y = b;

        let h = setup_walk(a, input, child, level + 1) + 1;
        if h > height {
            height = h;
        }
    }

    let n = &mut a[t];
    n.fam = n.axis.unwrap_or_else(|| input.rule.pick(height));

    height
}

fn third_walk(a: &mut Arena3, t: NodeId, dx: f64, dy: f64, dz: f64) {
    let n = &mut a[t];
    n.x -= dx;
    n.y -= dy;
    n.z -= dz;

    for i in 0..a.nchildren(t) {
        let c = a.child(t, i);
        third_walk(a, c, dx, dy, dz);
    }
}

/// Lays out the tree described by `input`, writing `x`, `y` and `z` into every node.
///
/// The drawing is normalized so that the smallest `x` and `z` end up at `input.x` and
/// `input.z`, and the root at `input.y`. Applying it twice to the same tree gives the
/// same coordinates.
pub fn layout3(arena: &mut Arena3, input: &LayoutInput3) {
    layout3_with(arena, input, &mut Callbacks3::default());
}

/// [`layout3`], with the two observation hooks.
pub fn layout3_with(arena: &mut Arena3, input: &LayoutInput3, cb: &mut Callbacks3) {
    let root = input.root;

    let on = crate::trace::enabled();
    let mut sum = std::time::Duration::ZERO;

    trace!(
        "layout3      root=#{} arena={} centered={} rule={:?} origin=({}, {}, {})",
        arena[root].idx,
        arena.len(),
        input.centered,
        input.rule,
        input.x,
        input.y,
        input.z
    );

    let t = crate::trace::start();
    let height = setup_walk(arena, input, root, 0);
    sum += phase!(t, "setup", "height={height}");

    let t = crate::trace::start();
    first_walk(arena, cb, root);
    sum += phase!(t, "first");

    let t = crate::trace::start();
    let [minx, minz] = second_walk(arena, input, cb, root, [0.0, 0.0]);
    sum += phase!(t, "second", "minx={minx} minz={minz}");

    let dx = minx - input.x;
    let dy = 0.0 - input.y;
    let dz = minz - input.z;

    let t = crate::trace::start();
    if dx != 0.0 || dy != 0.0 || dz != 0.0 {
        third_walk(arena, root, dx, dy, dz);
        sum += phase!(t, "third", "dx={dx} dy={dy} dz={dz}");
    } else {
        sum += phase!(t, "third", "already at the origin");
    }

    total!(on, sum);
}

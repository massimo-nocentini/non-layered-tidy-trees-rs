//! Non-layered tidy trees in three dimensions.
//!
//! Every node is a box `w` × `h` × `d`, its extents along `x`, `y` and `z`. One axis is the
//! depth axis — `y` by default, top down, exactly as in [`crate::layout`] — and stays
//! non-layered: a child starts at its parent's bottom. The other two are the *breadth*
//! axes, the plane the children of a node are spread over.
//!
//! The 2D algorithm is linear because merging a subtree into its left siblings is one
//! splice of two threaded contours. That needs the siblings to be ordered along a line,
//! so here every internal node `v` picks a *family axis* `a(v)`, one of the two breadth
//! axes, and
//!
//! * lays its children out along `a(v)` with the very same `separate`, sibling chain,
//!   `move_subtree` and `position_root` as [`crate::layout`], on the contours along `a(v)`;
//! * centres each child's box under its own box along the other breadth axis `b(v)`.
//!
//! Each subtree keeps four threaded contours, left and right along both breadth axes.
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
//! [`crate::layout`] draws the same tree, whatever the `d` of each box; with the depth
//! along `x` and every family along `y`, as it draws the tree with `vertically` off.
//!
//! [`flat`] computes the same coordinates with sweeps over a struct-of-arrays mirror,
//! which recurse nowhere.
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

pub mod flat;

/// One of the three axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Axis {
    /// Where a box measures `w`.
    X,
    /// Where a box measures `h`; the default depth axis.
    #[default]
    Y,
    /// Where a box measures `d`.
    Z,
}

impl Axis {
    /// The position of the axis in `x`, `y`, `z` order.
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }

    fn of(i: usize) -> Axis {
        [Axis::X, Axis::Y, Axis::Z][i]
    }
}

/// Which world axis plays which part: the depth axis, and the two breadth axes in `x`, `y`,
/// `z` order. The walks only ever see breadth axis 0 and 1 and the depth axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) breadth: [usize; 2],
    pub(crate) depth: usize,
}

impl Frame {
    pub(crate) fn new(depth: Axis) -> Frame {
        let d = depth.index();
        let mut breadth = [0; 2];
        let mut k = 0;
        for i in 0..3 {
            if i != d {
                breadth[k] = i;
                k += 1;
            }
        }
        Frame { breadth, depth: d }
    }

    /// Which breadth axis `ax` is, or `None` for the depth axis.
    fn slot(self, ax: Axis) -> Option<usize> {
        self.breadth.iter().position(|&b| b == ax.index())
    }

    /// The breadth axis a family is laid out along, for a node of children.
    ///
    /// # Panics
    ///
    /// If the axis asked for, by the node or by the rule, is the depth axis.
    pub(crate) fn family(self, axis: Option<Axis>, rule: AxisRule, height: usize) -> usize {
        let ax = axis.unwrap_or_else(|| rule.pick(height, self));
        self.slot(ax).unwrap_or_else(|| {
            panic!("a family cannot be laid out along {ax:?}, which is the depth axis")
        })
    }
}

/// Which axis each family is laid out along, for the nodes that do not say.
///
/// [`Node3::axis`] overrides the rule node by node. Either has to name a breadth axis: a
/// family along the depth axis is a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AxisRule {
    /// Every family along `x`.
    #[default]
    AllX,
    /// Every family along `y`, for a depth axis other than `y`.
    AllY,
    /// Every family along `z`.
    AllZ,
    /// By the height of the subtree: odd heights along the first breadth axis — `x`, or `y`
    /// when the depth is along `x` — even heights along the second.
    ///
    /// The parents of leaves only, height 1, spread along the first, their parents along
    /// the second, and so on upwards. The height depends on the subtree alone, so
    /// isomorphic subtrees get the same axes and come out congruent; a rule by level would
    /// not.
    Alternate,
}

impl AxisRule {
    fn pick(self, height: usize, frame: Frame) -> Axis {
        match self {
            AxisRule::AllX => Axis::X,
            AxisRule::AllY => Axis::Y,
            AxisRule::AllZ => Axis::Z,
            AxisRule::Alternate => Axis::of(frame.breadth[if height % 2 == 1 { 0 } else { 1 }]),
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
    /// Extent along `y`.
    pub h: f64,
    /// Extent along `z`.
    pub d: f64,
    /// Extra separation kept on the far side of the box along both breadth axes: from the
    /// sibling subtree that follows along the family axis, and as part of the hull along
    /// the other.
    pub margin: f64,
    /// The axis this node's children are laid out along; `None` leaves it to
    /// [`LayoutInput3::rule`].
    pub axis: Option<Axis>,
    /// Coordinate along `x`.
    pub x: f64,
    /// Coordinate along `y`.
    pub y: f64,
    /// Coordinate along `z`.
    pub z: f64,
    /// The parent, set by [`layout3`]; `None` for the root.
    pub parent: Option<NodeId>,

    children: Vec<NodeId>,
    /// The resolved family axis, as a breadth axis of the frame and as a world axis.
    fam: usize,
    family: Axis,

    // Everything below is in the frame of the layout: per breadth axis, indexed 0 and 1,
    // or along the depth axis. The world fields are only written by the second walk.
    ext: [f64; 2],
    dext: f64,
    top: f64,

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
    /// The subtree's extent as `[min0, max0, min1, max1]` over the breadth axes, relative
    /// to this node's own corner and with the margins on the far faces.
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
            fam: 0,
            family: Axis::X,
            ext: [0.0; 2],
            dext: 0.0,
            top: 0.0,
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
    pub fn family(&self) -> Axis {
        self.family
    }

    /// The extent along an axis.
    pub fn extent(&self, ax: Axis) -> f64 {
        [self.w, self.h, self.d][ax.index()]
    }

    /// The coordinate along an axis.
    pub fn coord(&self, ax: Axis) -> f64 {
        [self.x, self.y, self.z][ax.index()]
    }

    fn coord_mut(&mut self, i: usize) -> &mut f64 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            _ => &mut self.z,
        }
    }

    fn ext(&self, ax: usize) -> f64 {
        self.ext[ax]
    }

    /// The bottom along the depth axis, while the walks run.
    fn bot(&self) -> f64 {
        self.top + self.dext
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

    /// The far edge of a node's box along the depth axis, the counterpart of
    /// [`Arena::bottom`].
    pub fn bottom(&self, t: NodeId, depth: Axis) -> f64 {
        let n = &self.nodes[t.0];
        let e = n.extent(depth);
        n.coord(depth) + if n.centered { e / 2.0 } else { e }
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
            Item::Node(n) => self[n].bot(),
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
                    _ if n.fam == ax => match side {
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
    /// The depth axis, the counterpart of `vertically`.
    pub depth: Axis,
    /// Whether `x`/`y`/`z` should denote the middle of each box rather than its corner.
    pub centered: bool,
    /// The family axis of the nodes whose [`Node3::axis`] is `None`.
    pub rule: AxisRule,
    /// Where the drawing starts along `x`: its smallest `x`, or the root's when `x` is the
    /// depth axis. Likewise for `y` and `z`.
    pub x: f64,
    /// Where the drawing starts along `y`.
    pub y: f64,
    /// Where the drawing starts along `z`.
    pub z: f64,
}

impl LayoutInput3 {
    /// A top down drawing of `root` with every family along `x`, in corner coordinates,
    /// starting at the origin.
    pub fn new(root: NodeId) -> Self {
        LayoutInput3 {
            root,
            depth: Axis::Y,
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
pub type ContourPairsFn3<'a> = dyn FnMut(&Arena3, Item, Item, Axis, f64) + 'a;

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
    let ax = a[t].fam;
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
    frame: Frame,
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
            f(a, s, c, Axis::of(frame.breadth[ax]), dist);
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

fn first_walk(a: &mut Arena3, cb: &mut Callbacks3, frame: Frame, t: NodeId) {
    let k = a.nchildren(t);

    if k == 0 {
        // setting extremes, along both axes
        let n = &mut a[t];
        n.el = [Some(Item::Node(t)); 2];
        n.er = [Some(Item::Node(t)); 2];
        n.msel = [0.0; 2];
        n.mser = [0.0; 2];
        n.bbox = [0.0, n.ext[0] + n.margin, 0.0, n.ext[1] + n.margin];
        n.maxdepth = n.bot();
        return;
    }

    let ax = a[t].fam;
    let b = 1 - ax;

    // Along the family axis: the 2D first walk, verbatim.
    let c0 = a.child(t, 0);
    first_walk(a, cb, frame, c0);

    let mut chain = Vec::new();
    let el = a[c0].el[ax].expect("the first walk sets the extreme left item");
    update_chain(&mut chain, a.item_bottom(el), 0);

    for i in 1..k {
        let ci = a.child(t, i);
        first_walk(a, cb, frame, ci);

        let er = a[ci].er[ax].expect("the first walk sets the extreme right item");
        let min = a.item_bottom(er);

        separate(a, cb, frame, t, i, &chain, ax);

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
    let mut bbox = [0.0, n.ext[0] + n.margin, 0.0, n.ext[1] + n.margin];
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

    let n = &mut a[t];
    let bottom = n.bot();
    n.bbox = bbox;
    n.maxdepth = if d1 > bottom { d1 } else { bottom };
}

fn second_walk(
    a: &mut Arena3,
    input: &LayoutInput3,
    cb: &mut Callbacks3,
    frame: Frame,
    t: NodeId,
    modsum_init: [f64; 2],
) -> [f64; 2] {
    let n = &mut a[t];

    let modsum = [
        modsum_init[0] + n.modifier[0],
        modsum_init[1] + n.modifier[1],
    ];
    let d0 = n.prelim[0] + modsum[0];
    let d1 = n.prelim[1] + modsum[1];

    let (o0, od, o1) = if input.centered {
        (n.ext[0] / 2.0, n.dext / 2.0, n.ext[1] / 2.0)
    } else {
        (0.0, 0.0, 0.0)
    };
    n.centered = input.centered;

    let best = [d0 + o0, d1 + o1];
    *n.coord_mut(frame.breadth[0]) = best[0];
    *n.coord_mut(frame.breadth[1]) = best[1];
    *n.coord_mut(frame.depth) = n.top + od;
    let mut best = best;

    add_child_spacing(a, t);

    if let Some(f) = cb.walk.as_mut() {
        f(a, t);
    }

    for i in 0..a.nchildren(t) {
        let c = a.child(t, i);
        let current = second_walk(a, input, cb, frame, c, modsum);
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
fn setup_walk(
    a: &mut Arena3,
    input: &LayoutInput3,
    frame: Frame,
    t: NodeId,
    level: usize,
) -> usize {
    let n = &mut a[t];

    n.level = level;
    n.centered = false;

    let world = [n.w, n.h, n.d];
    n.ext = [world[frame.breadth[0]], world[frame.breadth[1]]];
    n.dext = world[frame.depth];

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
        n.top = 0.0;
    }

    let b = n.bot();
    let mut height = 0;

    for i in 0..a.nchildren(t) {
        let child = a.child(t, i);
        let c = &mut a[child];

        c.childno = i;
        c.parent = Some(t);
        c.top = b;

        let h = setup_walk(a, input, frame, child, level + 1) + 1;
        if h > height {
            height = h;
        }
    }

    let n = &mut a[t];
    n.fam = if n.children.is_empty() {
        0
    } else {
        frame.family(n.axis, input.rule, height)
    };
    n.family = Axis::of(frame.breadth[n.fam]);

    height
}

fn third_walk(a: &mut Arena3, t: NodeId, delta: [f64; 3]) {
    let n = &mut a[t];
    n.x -= delta[0];
    n.y -= delta[1];
    n.z -= delta[2];

    for i in 0..a.nchildren(t) {
        let c = a.child(t, i);
        third_walk(a, c, delta);
    }
}

/// What the third walk subtracts along `x`, `y` and `z`: the breadth axes are normalized to
/// the origin first, the depth axis is only offset.
pub(crate) fn deltas(frame: Frame, mins: [f64; 2], origin: [f64; 3]) -> [f64; 3] {
    let mut delta = [0.0; 3];
    delta[frame.breadth[0]] = mins[0] - origin[frame.breadth[0]];
    delta[frame.breadth[1]] = mins[1] - origin[frame.breadth[1]];
    delta[frame.depth] = 0.0 - origin[frame.depth];
    delta
}

/// Lays out the tree described by `input`, writing `x`, `y` and `z` into every node.
///
/// The drawing is normalized so that the smallest coordinate along each breadth axis ends
/// up at the origin `input` gives, and the root at its coordinate along the depth axis.
/// Applying it twice to the same tree gives the same coordinates.
///
/// # Panics
///
/// If a node with children asks for its family along the depth axis, through its own
/// [`Node3::axis`] or through the rule.
pub fn layout3(arena: &mut Arena3, input: &LayoutInput3) {
    layout3_with(arena, input, &mut Callbacks3::default());
}

/// [`layout3`], with the two observation hooks.
pub fn layout3_with(arena: &mut Arena3, input: &LayoutInput3, cb: &mut Callbacks3) {
    let root = input.root;
    let frame = Frame::new(input.depth);

    let on = crate::trace::enabled();
    let mut sum = std::time::Duration::ZERO;

    trace!(
        "layout3      root=#{} arena={} depth={:?} centered={} rule={:?} origin=({}, {}, {})",
        arena[root].idx,
        arena.len(),
        input.depth,
        input.centered,
        input.rule,
        input.x,
        input.y,
        input.z
    );

    let t = crate::trace::start();
    let height = setup_walk(arena, input, frame, root, 0);
    sum += phase!(t, "setup", "height={height}");

    let t = crate::trace::start();
    first_walk(arena, cb, frame, root);
    sum += phase!(t, "first");

    let t = crate::trace::start();
    let mins = second_walk(arena, input, cb, frame, root, [0.0, 0.0]);
    sum += phase!(t, "second", "min={mins:?}");

    let delta = deltas(frame, mins, [input.x, input.y, input.z]);

    let t = crate::trace::start();
    if delta.iter().any(|&d| d != 0.0) {
        third_walk(arena, root, delta);
        sum += phase!(t, "third", "delta={delta:?}");
    } else {
        sum += phase!(t, "third", "already at the origin");
    }

    total!(on, sum);
}

/// How to lay out a tree given as arrays; [`LayoutInput3`] without the root.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ApiInput3 {
    /// The depth axis.
    pub depth: Axis,
    /// Whether the coordinates should denote the middle of each box rather than its corner.
    pub centered: bool,
    /// The family axis of every node; the heights [`AxisRule::Alternate`] goes by are the
    /// tree's own, the gap nodes left out.
    pub rule: AxisRule,
    /// Where the drawing starts along `x`.
    pub x: f64,
    /// Where the drawing starts along `y`.
    pub y: f64,
    /// Where the drawing starts along `z`.
    pub z: f64,
}

/// Where the children of every node start in the adjacency of `children`.
pub(crate) fn offsets(n: usize, children: &[usize]) -> Vec<usize> {
    assert!(children.len() >= n, "`children` starts with {n} counts");
    let mut off = Vec::with_capacity(n);
    let mut acc = n;
    for &count in &children[..n] {
        off.push(acc);
        acc += count;
    }
    assert!(
        children.len() >= acc,
        "the adjacency is shorter than the counts promise"
    );
    off
}

/// The height of every node the root reaches, gap nodes left out; 0 elsewhere.
pub(crate) fn heights(n: usize, children: &[usize], off: &[usize], rooti: usize) -> Vec<usize> {
    let mut order = Vec::with_capacity(n);
    let mut stack = vec![rooti - 1];
    while let Some(s) = stack.pop() {
        order.push(s);
        stack.extend((0..children[s]).map(|k| children[off[s] + k] - 1));
    }

    let mut height = vec![0; n];
    for &s in order.iter().rev() {
        for k in 0..children[s] {
            let c = children[off[s] + k] - 1;
            if height[c] + 1 > height[s] {
                height[s] = height[c] + 1;
            }
        }
    }
    height
}

/// Builds the arena of the flat description used by [`layout_api3`], the counterpart of
/// [`crate::reify_flat_chunks`].
///
/// Every node gets an invisible gap node above it, with no margin: node `i` is `nodes[i]`
/// and its gap node `nodes[i + n]`, so the root of the tree is `nodes[rooti - 1 + n]`. The
/// family axes are resolved here, by the rule and the heights of the tree without its gap
/// nodes, and set on every node; a gap node takes the axis of the node below it.
///
/// # Panics
///
/// If the arrays are shorter than the layouts documented in [`layout_api3`], or if the rule
/// asks for a family along the depth axis.
pub fn reify3(
    n: usize,
    whd: &[f64],
    gaps: &[f64],
    children: &[usize],
    rooti: usize,
    input: &ApiInput3,
) -> (Arena3, Vec<NodeId>) {
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
    let frame = Frame::new(input.depth);

    let mut arena = Arena3::with_capacity(2 * n);
    let mut nodes = Vec::with_capacity(2 * n);

    for i in 0..n {
        nodes.push(arena.add_node(i + 1, whd[i], whd[i + n], whd[i + 2 * n], whd[i + 3 * n]));
    }

    for i in 0..n {
        let gap = arena.add_node(i + 1 + n, gaps[i], gaps[i + n], gaps[i + 2 * n], 0.0);
        arena.push_child(gap, nodes[i]);
        nodes.push(gap);
    }

    for i in 0..n {
        for k in 0..children[i] {
            let c = children[off[i] + k];
            arena.push_child(nodes[i], nodes[c - 1 + n]);
        }

        let fam = if children[i] == 0 {
            0
        } else {
            frame.family(None, input.rule, height[i])
        };
        let ax = Some(Axis::of(frame.breadth[fam]));
        arena[nodes[i]].axis = ax;
        arena[nodes[i + n]].axis = ax;
    }

    (arena, nodes)
}

/// Lays out a tree given as parallel arrays, the counterpart of [`crate::layout_api`].
///
/// * `whd` — `4n` values, `[w_0..w_n-1, h_0.., d_0.., margin_0..]`; on return the first `3n`
///   are overwritten with `[x_0.., y_0.., z_0..]`
/// * `gaps` — `3n` values, the `w`, `h` and `d` of the invisible node inserted above every
///   node, which is what spaces the levels apart; it keeps no margin
/// * `children` — `n` counts followed by the adjacency, one-based, grouped by parent
/// * `rooti` — the one-based index of the root
///
/// A node the root does not reach is left at the origin.
///
/// # Panics
///
/// If the arrays are shorter than that, if `rooti` is out of range, or if the rule asks
/// for a family along the depth axis.
pub fn layout_api3(
    n: usize,
    whd: &mut [f64],
    gaps: &[f64],
    children: &[usize],
    rooti: usize,
    input: &ApiInput3,
) {
    trace!(
        "layout_api3  n={n} rooti={rooti} depth={:?} centered={} rule={:?} origin=({}, {}, {})",
        input.depth,
        input.centered,
        input.rule,
        input.x,
        input.y,
        input.z
    );

    let t = crate::trace::start();
    let (mut arena, nodes) = reify3(n, whd, gaps, children, rooti, input);
    phase!(t, "reify", "arena={}", arena.len());

    layout3(
        &mut arena,
        &LayoutInput3 {
            root: nodes[rooti - 1 + n],
            depth: input.depth,
            centered: input.centered,
            rule: input.rule,
            x: input.x,
            y: input.y,
            z: input.z,
        },
    );

    let t = crate::trace::start();
    for (i, &v) in nodes[..n].iter().enumerate() {
        let node = &arena[v];
        whd[i] = node.x;
        whd[i + n] = node.y;
        whd[i + 2 * n] = node.z;
    }
    phase!(t, "write", "{n} coordinate triples");
}

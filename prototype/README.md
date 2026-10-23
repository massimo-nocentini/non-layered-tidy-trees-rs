# The 3D prototype

`proto.py` is the exact-arithmetic Python model of `src/space.rs` that the design review
used before any Rust existed: contours are explicit lists rather than threads, offsets
are `Fraction`s, and middle-sibling spreading is applied directly. It is the reference
`space::layout3` is checked against, not part of the crate.

```sh
python3 prototype/xcheck.py          # layout3 against the prototype, 2 000 random trees
python3 prototype/t_overlap.py       # the prototype's own overlap and margin check
python3 prototype/t_lin.py           # its contour steps per node on adversarial shapes
```

`xcheck.py` hands the prototype's choice of family axis to the Rust side node by node, so
the two lay out exactly the same problem; `examples/space_xcheck.rs` is the Rust half.
`t_lin.py` also measures the `trial` rule, which tries both axes at every node and which
the crate does not implement.

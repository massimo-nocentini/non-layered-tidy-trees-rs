# Contributing

Thanks for looking. This is a port, and that shapes what a good patch looks like here.

## The one rule

The crate is meant to be **numerically identical** to
[`non-layered-tidy-trees.c`](https://github.com/massimo-nocentini/non-layered-tidy-trees.c),
not merely equivalent: the walks, the sibling chain, the threading and the one behavioural
fix in `separate` follow the C line by line, in the same order, so the two produce
bit-identical coordinates. A patch that reorders a floating point expression changes the
output even when the algebra says otherwise, and that is a breaking change, not a cleanup.

The same holds between the two implementations in the crate: `layout` and
`flat::layout_flat` have to agree bit for bit, which is what `tests/flat.rs` and the bench
checksums check.

## Getting set up

No dependencies, so there is nothing to install beyond a stable toolchain.

```sh
make test        # the whole suite
make release     # the optimized build of the library and both binaries
make bench       # the tables of the README
make doc         # rustdoc into ./docs
make help        # every target
```

`NLTT_TRIALS` sets the trees per shape (default 300, the C default) and `NLTT_DEPTH` the
depth of the chain used by `tests/depth.rs`: `make test NLTT_TRIALS=25` for a quicker sweep
over the same shapes.

## Before opening a pull request

* `make test` passes, both feature modes.
* `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features` are clean.
  CI runs with `-D warnings`, so a warning is a failure.
* If you touched `src/lib.rs` or `src/flat.rs`, the bench checksums are unchanged. They are
  the last column of `cargo run --release --bin bench -- 7`, the XOR of the bit patterns of
  every coordinate: exact and order independent, so a single wrong coordinate anywhere in a
  million-node tree changes them.
* If you can build the C sources, run the differential oracle as well — it compares
  9 946 368 coordinates over 115 200 trees against what the C build printed:

  ```sh
  make -C ../test overlap
  ../test/overlap --dump 300 > golden.txt
  cargo run --release --bin dump -- --check golden.txt 300
  ```

* If you changed performance, put the before and after numbers in the pull request, taken
  the same way on the same machine. The README's tables were measured on a Xeon Gold 6238R
  and are not comparable with a run on your laptop.

## Style

Match what is there. The sources are commented in prose, the comments say *why* a step is
the way it is — usually a pointer back to the C or to the paper — and the C names are kept
alongside the Rust ones so the two can be read side by side. `mod` became `modifier`
because it is a keyword, and that is the kind of departure that gets a comment.

## The generated documentation

`docs/` is rustdoc output, regenerated with `make doc` and committed so GitHub Pages can
serve it from the folder. Do not hand-edit it; if a patch changes the public API, run
`make doc` and include the result.

## Licence

By contributing you agree that your work is licensed under the [MIT licence](LICENSE) that
covers this repository.

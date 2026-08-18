## What this changes

<!-- One paragraph. What the patch does, and why. -->

## Checklist

- [ ] `make test` passes — the suite runs with the scalar kernels and then with `--features simd`, and both have to pass identically
- [ ] `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features` are clean
- [ ] The coordinates are unchanged, or the change to them is deliberate and explained above

## If this touches the layout

The port is meant to be numerically identical to the C, so say which of these you ran:

- [ ] The bench checksums match the ones from before the change (`cargo run --release --bin bench -- 7`, last column)
- [ ] The differential oracle passes against a dump from the C build (`cargo run --release --bin dump -- --check golden.txt 300`)
- [ ] Neither applies, because this does not touch `src/lib.rs` or `src/flat.rs`

## If this touches performance

Numbers from before and after, taken the same way (`make bench`), on the machine you ran them on.

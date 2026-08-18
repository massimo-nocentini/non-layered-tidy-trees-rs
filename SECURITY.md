# Security policy

## Supported versions

The latest release on the `master` branch is the only supported version.

## Reporting a vulnerability

Report privately through GitHub's
[security advisories](https://github.com/massimo-nocentini/non-layered-tidy-trees-rs/security/advisories/new),
or by email to <massimo.nocentini@gmail.com>. Please do not open a public issue for a
vulnerability. Expect an acknowledgement within a week.

## What is in scope

This is a computational geometry library with no dependencies, no I/O and no network
access: it takes node dimensions and a parent-child relation and writes coordinates back.
The realistic failure modes are therefore

* a panic, an out-of-bounds index or an arithmetic overflow reachable from input that the
  documented API accepts — including the array layouts of `layout_api`, whose contents come
  from the caller and are validated only by the documented `assert!`s;
* a stack overflow from a deep tree, which the recursive walks can reach; `tests/depth.rs`
  documents the depth the crate is exercised at and the stack size it needs;
* any unsoundness in the AVX2 kernels behind `--features simd`, which are the only `unsafe`
  in the crate. They are off by default, and with the feature off there is no `unsafe`
  anywhere in the sources.

Reports of the first and third kinds are welcome even without a proof of exploitability — a
tree that panics is enough.

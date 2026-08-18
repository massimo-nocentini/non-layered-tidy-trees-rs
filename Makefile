# Drawing non-layered tidy trees in linear time -- the commands the README quotes.
#
#   make test      the suite, scalar kernels and then the vectorized ones
#   make release   the optimized build of the library and both binaries
#   make bench     the tables of the README's "Benchmarks" section
#   make doc       the API into ./docs, ready to serve as a GitHub Pages folder
#
# `make help` lists every target.

CARGO   ?= cargo
CRATE    = non_layered_tidy_trees
DOCS     = docs
# The README's numbers are best of 7, with the AVX2 kernels in the table.
REPS    ?= 7
FEATURES = --features simd

.PHONY: all help doc release build test test-simd bench bench-phases clean distclean

all: release

help:
	@sed -n 's/^#   //p' $(firstword $(MAKEFILE_LIST))

## doc -- rustdoc into ./docs
#
# `--all-features` so the `simd` items are documented too; they are part of the API
# even when the default build does not compile them. The redirect at the root is what
# makes the folder browsable as a site: rustdoc puts the crate one level down.
doc:
	$(CARGO) doc --no-deps --all-features
	rm -rf $(DOCS)
	cp -R target/doc $(DOCS)
	printf '<meta http-equiv="refresh" content="0;url=%s/index.html">\n' $(CRATE) > $(DOCS)/index.html
	touch $(DOCS)/.nojekyll
	@echo "docs in ./$(DOCS) -- open $(DOCS)/$(CRATE)/index.html"

## release -- the optimized build, library and binaries, with and without SIMD
release:
	$(CARGO) build --release
	$(CARGO) build --release $(FEATURES)

build:
	$(CARGO) build

## test -- the whole suite; the vectorized kernels have to pass identically
#
# NLTT_TRIALS sets the trees per shape (default 300) and NLTT_DEPTH the depth of the
# chain: `make test NLTT_TRIALS=25` for the quicker sweep.
test:
	$(CARGO) test
	$(MAKE) test-simd

test-simd:
	$(CARGO) test $(FEATURES)

## bench -- the two tables of the README, best of $(REPS)
#
# One tree of n nodes laid out repeatedly, the tree built outside the timed region and
# the first layout thrown away. `make bench REPS=3` for a quicker, noisier pass.
bench:
	$(CARGO) run --release $(FEATURES) --bin bench -- $(REPS)

## bench-phases -- where the time goes inside one flat layout (the third README table)
bench-phases:
	$(CARGO) run --release $(FEATURES) --bin bench -- $(REPS) --phases

clean:
	$(CARGO) clean

## distclean -- clean, and drop the generated ./docs as well
distclean: clean
	rm -rf $(DOCS)

# Drawing non-layered tidy trees in linear time -- the commands the README quotes.
#
#   make test      the whole suite
#   make release   the optimized build of the library and both binaries
#   make bench     the tables of the README's "Benchmarks" section
#   make trace     one tree through every entry point, with NLTT_TRACE on
#   make doc       the API into ./docs, ready to serve as a GitHub Pages folder
#
# `make help` lists every target.

CARGO   ?= cargo
CRATE    = non_layered_tidy_trees
DOCS     = docs
# The README's numbers are best of 7.
REPS    ?= 7

.PHONY: all help doc release build test bench bench-phases trace clean distclean

all: release

help:
	@sed -n 's/^#   //p' $(firstword $(MAKEFILE_LIST))

## doc -- rustdoc into ./docs
#
# The redirect at the root is what makes the folder browsable as a site: rustdoc puts
# the crate one level down.
doc:
	$(CARGO) doc --no-deps
	rm -rf $(DOCS)
	cp -R target/doc $(DOCS)
	printf '<meta http-equiv="refresh" content="0;url=%s/index.html">\n' $(CRATE) > $(DOCS)/index.html
	touch $(DOCS)/.nojekyll
	@echo "docs in ./$(DOCS) -- open $(DOCS)/$(CRATE)/index.html"

## release -- the optimized build, library and binaries
release:
	$(CARGO) build --release

build:
	$(CARGO) build

## test -- the whole suite
#
# NLTT_TRIALS sets the trees per shape (default 300) and NLTT_DEPTH the depth of the
# chain: `make test NLTT_TRIALS=25` for the quicker sweep.
test:
	$(CARGO) test

## bench -- the two tables of the README, best of $(REPS)
#
# One tree of n nodes laid out repeatedly, the tree built outside the timed region and
# the first layout thrown away. `make bench REPS=3` for a quicker, noisier pass.
bench:
	$(CARGO) run --release --bin bench -- $(REPS)

## bench-phases -- where the time goes inside one flat layout (the third README table)
bench-phases:
	$(CARGO) run --release --bin bench -- $(REPS) --phases

## trace -- what NLTT_TRACE prints, over one small tree
#
# Any run of any binary takes the same variable; this is just the smallest example.
trace:
	NLTT_TRACE=1 $(CARGO) run --example trace

clean:
	$(CARGO) clean

## distclean -- clean, and drop the generated ./docs as well
distclean: clean
	rm -rf $(DOCS)

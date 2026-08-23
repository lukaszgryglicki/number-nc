# NF/NC arbitrary-precision calculator - build driver.
#
#   make        - optimized release build (binary: target/release/nc)
#   make debug  - debug build (binary: target/debug/nc)
#   make test   - run the full test suite (lib + integration)
#   make clean  - remove all build artifacts

CARGO ?= cargo

.PHONY: all release debug test clean

all: release

release:
	$(CARGO) build --release
	@echo "binary: target/release/nc"

debug:
	$(CARGO) build
	@echo "binary: target/debug/nc"

test:
	$(CARGO) test

clean:
	$(CARGO) clean

# NF/NC arbitrary-precision calculator - build driver.
#
#   make           - optimized release build (binary: target/release/nc)
#   make debug     - debug build (binary: target/debug/nc)
#   make test      - run the full test suite (lib + integration)
#   make test N=15 - the same suite, additionally run at precision N=15
#                    (also as env var: N=15 make test)
#   make confirm32 - in-memory random-add confirmation at N=32 (~8 GiB RAM,
#                    prints states only; also: make confirm32 N=30)
#   make clean     - remove all build artifacts

CARGO ?= cargo
N ?=

.PHONY: all release debug test confirm32 clean

all: release

release:
	$(CARGO) build --release
	@echo "binary: target/release/nc"

debug:
	$(CARGO) build
	@echo "binary: target/debug/nc"

test:
	NF_TEST_N=$(N) $(CARGO) test

confirm32:
	$(CARGO) run --release --example confirm32 $(if $(N),$(N),32)

clean:
	$(CARGO) clean

# NF/NC arbitrary-precision calculator - build driver.
#
#   make           - optimized release build (binary: target/release/nc)
#   make debug     - debug build (binary: target/debug/nc)
#   make test      - run the full test suite (lib + integration)
#   make test N=15 - the same suite, additionally run at precision N=15
#                    (also as env var: N=15 make test)
#   make test ALL0F=1 - additionally run the exhaustive 0F suite: all
#                    64x64 raw 6-bit encodings x {add,sub,mul,div,pow}
#                    = 20480 cases, bit-exact against an exact oracle
#   make test ALL1F=1 - the same for 1F: all 256x256 raw 8-bit encodings
#                    x 5 ops = 327680 bit-exact cases
#   make confirm32 - in-memory random-add confirmation at N=32 (~8 GiB RAM,
#                    prints states only; also: make confirm32 N=30)
#   make clean     - remove all build artifacts

CARGO ?= cargo
N ?=
ALL0F ?=
ALL1F ?=

.PHONY: all release debug test confirm32 clean

all: release

release:
	$(CARGO) build --release
	@echo "binary: target/release/nc"

debug:
	$(CARGO) build
	@echo "binary: target/debug/nc"

test:
	NF_TEST_N=$(N) NF_ALL0F=$(ALL0F) NF_ALL1F=$(ALL1F) $(CARGO) test

confirm32:
	$(CARGO) run --release --example confirm32 $(if $(N),$(N),32)

clean:
	$(CARGO) clean

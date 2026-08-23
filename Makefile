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
#   make test ALL2F=1 - exhaustive 2F suite: all 4096x4096 raw 12-bit
#                    encoding pairs x {add,mul,div,pow} = 67108864
#                    bit-exact cases (+ 4096 encoding checks)
#   make test ALL0C=1 - exhaustive 0C complex suite: all 4096x4096 raw
#                    12-bit complex encoding pairs x {add,mul,div,pow}
#                    = 67108864 bit-exact cases (+ 4096 encoding checks)
#   make confirm32 - in-memory random-add confirmation at N=32 (~8 GiB RAM,
#                    prints states only; also: make confirm32 N=30)
#   make clean     - remove all build artifacts

CARGO ?= cargo
N ?=
ALL0F ?=
ALL1F ?=
ALL2F ?=
ALL0C ?=

.PHONY: all release debug test confirm32 clean

all: release

release:
	$(CARGO) build --release
	@echo "binary: target/release/nc"

debug:
	$(CARGO) build
	@echo "binary: target/debug/nc"

test:
	NF_TEST_N=$(N) NF_ALL0F=$(ALL0F) NF_ALL1F=$(ALL1F) NF_ALL2F=$(ALL2F) NF_ALL0C=$(ALL0C) $(CARGO) test

confirm32:
	$(CARGO) run --release --example confirm32 $(if $(N),$(N),32)

clean:
	$(CARGO) clean

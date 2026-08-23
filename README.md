# NF / NC — arbitrary-precision fixed-point numbers with a calculator CLI

`nf` is a dependency-free Rust library implementing the **NF** number format —
an arbitrary-precision, fixed-point binary format parameterized by a single
number `N` — plus **NC**, its complex-number extension, plus `nc`, a
command-line calculator that evaluates complex math expressions at any
precision.

```console
$ make            # release build (or: cargo build --release)
$ ./target/release/nc 7 "(3.14159+2.73i)^(-2.00000000000000000000000000000000000000000000000000051-0.0000000000000000000000000000000001i)"
0.008053784635898781981668995528076383128-0.057163870131559166207370947318599216665i
```

Build targets (`Makefile` wraps cargo): `make` — optimized release build,
`make debug` — debug build, `make test` — full test suite
(`make test N=15` additionally runs it at precision 15,
`make test ALL0F=1` / `ALL1F=1` add the exhaustive bit-exact 0F/1F suites,
348 160 cases total, and `ALL2F=1` / `ALL0C=1` add the exhaustive 12-bit
2F real and 0C complex grids — 67 108 864 bit-exact cases each),
`make confirm32` — in-memory random-add proof at N=32, `make clean` —
remove artifacts.

## The NF format

An `NF` number with parameter `N` ("`NF`" = *N-Float*) is a signed
fixed-point binary number with the decimal (binary) point exactly in the
middle:

* `2^N` bits **before** the point (integer part),
* `2^N` bits **after** the point (fraction part),
* **4 flag bits**, always present:

| bit | name         | meaning when set                                                          |
|-----|--------------|---------------------------------------------------------------------------|
| 1st | **NaN**      | the number is invalid (result of an undefined operation)                  |
| 2nd | **Inf**      | overflow: the true magnitude exceeds the largest representable value      |
| 3rd | **InfSmall** | underflow: the true magnitude is non-zero but below the smallest positive |
| 4th | **Sign**     | `0` = positive, `1` = negative                                            |

One extra flag combination is meaningful: **Inf and InfSmall set together**
encodes **±Any** — *"sign known, magnitude completely indeterminate,
somewhere in `(0, +Inf]`"*. See [the `Any` state](#the-any-state) below.

Total logical size: `2·2^N + 4` bits.

Layout examples: `0F` = `FFFF b.b` (6 bits), `1F` = `FFFF bb.bb` (8),
`2F` = `FFFF bbbb.bbbb` (12), `3F` = `FFFF b^8.b^8` (20),
`10F` = `FFFF b^1024.b^1024` (2052).

### Capacity, memory and tetration reach, N = 0…50

| N | logical bits | RAM/value¹ | smallest positive² | largest value³ | max h: tet(2,h)⁴ | max h: tet(e,h)⁴ |
|---|---|---|---|---|---|---|
| 0 | 6 | 16 B | 2^−1 = 0.5 | 1.5 | 0 | 0 |
| 1 | 8 | 16 B | 2^−2 = 0.25 | 3.75 | 1 | 1 |
| 2 | 12 | 16 B | 2^−4 = 0.0625 | 15.9375 | 2 | 2 |
| 3 | 20 | 16 B | 2^−8 ≈ 0.0039 | 255.99609375 | 3 | 2 |
| 4 | 36 | 16 B | 2^−16 ≈ 1.5·10^−5 | ≈ 2^16 ≈ 6.6·10^4 | 3 | 2 |
| 5 | 68 | 16 B | 2^−32 ≈ 2.3·10^−10 | ≈ 2^32 ≈ 4.3·10^9 | 4 | 3 |
| 6 | 132 | 24 B | 2^−64 ≈ 5.4·10^−20 | ≈ 2^64 ≈ 1.8·10^19 | 4 | 3 |
| 7 | 260 | 40 B | 2^−128 ≈ 2.9·10^−39 | ≈ 2^128 ≈ 3.4·10^38 | 4 | 3 |
| 8 | 516 | 72 B | 2^−256 ≈ 8.6·10^−78 | ≈ 2^256 ≈ 1.2·10^77 | 4 | 3 |
| 9 | 1 028 | 136 B | 2^−512 ≈ 7.5·10^−155 | ≈ 2^512 ≈ 1.3·10^154 | 4 | 3 |
| 10 | 2 052 | 264 B | 2^−1024 ≈ 5.6·10^−309 | ≈ 2^1024 ≈ 1.8·10^308 | 4 | 3 |
| 11 | 4 100 | 520 B | 2^−2048 ≈ 3.1·10^−617 | ≈ 2^2048 ≈ 3.2·10^616 | 4 | 3 |
| 12 | 8 196 | 1 032 B | 2^−4096 ≈ 9.6·10^−1234 | ≈ 2^4096 ≈ 1.0·10^1233 | 4 | 3 |
| 13 | 16 388 | 2 056 B | 2^−8192 ≈ 9.2·10^−2467 | ≈ 2^8192 ≈ 1.1·10^2466 | 4 | 3 |
| 14 | 32 772 | 4 104 B | 2^−16384 ≈ 8.4·10^−4933 | ≈ 2^16384 ≈ 1.2·10^4932 | 4 | 3 |
| 15 | 65 540 | 8 200 B | 2^−32768 ≈ 7.1·10^−9865 | ≈ 2^32768 ≈ 1.4·10^9864 | 4 | 3 |
| 16 | 131 076 | 16 KiB | 2^−65536 ≈ 5.0·10^−19729 | ≈ 2^65536 ≈ 2.0·10^19728 | 4 | 3 |
| 17 | 262 148 | 32 KiB | 2^−131072 ≈ 2.5·10^−39457 | ≈ 2^131072 ≈ 4.0·10^39456 | 5 | 3 |
| 18 | 524 292 | 64 KiB | 2^−262144 ≈ 6.2·10^−78914 | ≈ 2^262144 ≈ 1.6·10^78913 | 5 | 3 |
| 19 | 1 048 580 | 128 KiB | 2^−524288 ≈ 3.9·10^−157827 | ≈ 2^524288 ≈ 2.6·10^157826 | 5 | 3 |
| 20 | 2 097 156 | 256 KiB | 2^−1048576 ≈ 1.5·10^−315653 | ≈ 2^1048576 ≈ 6.7·10^315652 | 5 | 3 |
| 21 | 4 194 308 | 512 KiB | 2^−2097152 ≈ 2.2·10^−631306 | ≈ 2^2097152 ≈ 4.5·10^631305 | 5 | 3 |
| 22 | 8 388 612 | 1 MiB | 2^−4 194 304 ≈ 10^(−1.26·10^6) | ≈ 2^4 194 304 ≈ 10^(1.26·10^6) | 5 | 3 |
| 23 | 16 777 220 | 2 MiB | 2^−8 388 608 ≈ 10^(−2.53·10^6) | ≈ 2^8 388 608 ≈ 10^(2.53·10^6) | 5 | 4 |
| 24 | 33 554 436 | 4 MiB | 2^−16 777 216 ≈ 10^(−5.05·10^6) | ≈ 2^16 777 216 ≈ 10^(5.05·10^6) | 5 | 4 |
| 25 | 67 108 868 | 8 MiB | 2^−33 554 432 ≈ 10^(−1.01·10^7) | ≈ 2^33 554 432 ≈ 10^(1.01·10^7) | 5 | 4 |
| 26 | 134 217 732 | 16 MiB | 2^−67 108 864 ≈ 10^(−2.02·10^7) | ≈ 2^67 108 864 ≈ 10^(2.02·10^7) | 5 | 4 |
| 27 | 268 435 460 | 32 MiB | 2^−134 217 728 ≈ 10^(−4.04·10^7) | ≈ 2^134 217 728 ≈ 10^(4.04·10^7) | 5 | 4 |
| 28 | 536 870 916 | 64 MiB | 2^−268 435 456 ≈ 10^(−8.08·10^7) | ≈ 2^268 435 456 ≈ 10^(8.08·10^7) | 5 | 4 |
| 29 | 1 073 741 828 | 128 MiB | 2^−536 870 912 ≈ 10^(−1.62·10^8) | ≈ 2^536 870 912 ≈ 10^(1.62·10^8) | 5 | 4 |
| 30 | 2 147 483 652 | 256 MiB | 2^−1 073 741 824 ≈ 10^(−3.23·10^8) | ≈ 2^1 073 741 824 ≈ 10^(3.23·10^8) | 5 | 4 |
| 31 | 4 294 967 300 | 512 MiB | 2^−2 147 483 648 ≈ 10^(−6.46·10^8) | ≈ 2^2 147 483 648 ≈ 10^(6.46·10^8) | 5 | 4 |
| 32 | 8 589 934 596 | 1 GiB | 2^−4 294 967 296 ≈ 10^(−1.29·10^9) | ≈ 2^4 294 967 296 ≈ 10^(1.29·10^9) | 5 | 4 |
| 33 | 17 179 869 188 | 2 GiB | 2^−8 589 934 592 ≈ 10^(−2.59·10^9) | ≈ 2^8 589 934 592 ≈ 10^(2.59·10^9) | 5 | 4 |
| 34 | 34 359 738 372 | 4 GiB | 2^−17 179 869 184 ≈ 10^(−5.17·10^9) | ≈ 2^17 179 869 184 ≈ 10^(5.17·10^9) | 5 | 4 |
| 35 | 68 719 476 740 | 8 GiB | 2^−34 359 738 368 ≈ 10^(−1.03·10^10) | ≈ 2^34 359 738 368 ≈ 10^(1.03·10^10) | 5 | 4 |
| 36 | 137 438 953 476 | 16 GiB | 2^−68 719 476 736 ≈ 10^(−2.07·10^10) | ≈ 2^68 719 476 736 ≈ 10^(2.07·10^10) | 5 | 4 |
| 37 | 274 877 906 948 | 32 GiB | 2^−137 438 953 472 ≈ 10^(−4.14·10^10) | ≈ 2^137 438 953 472 ≈ 10^(4.14·10^10) | 5 | 4 |
| 38 | 549 755 813 892 | 64 GiB | 2^−274 877 906 944 ≈ 10^(−8.27·10^10) | ≈ 2^274 877 906 944 ≈ 10^(8.27·10^10) | 5 | 4 |
| 39 | 1 099 511 627 780 | 128 GiB | 2^−549 755 813 888 ≈ 10^(−1.65·10^11) | ≈ 2^549 755 813 888 ≈ 10^(1.65·10^11) | 5 | 4 |
| 40 | 2 199 023 255 556 | 256 GiB | 2^−1 099 511 627 776 ≈ 10^(−3.31·10^11) | ≈ 2^1 099 511 627 776 ≈ 10^(3.31·10^11) | 5 | 4 |
| 41 | 4 398 046 511 108 | 512 GiB | 2^−2 199 023 255 552 ≈ 10^(−6.62·10^11) | ≈ 2^2 199 023 255 552 ≈ 10^(6.62·10^11) | 5 | 4 |
| 42 | 8 796 093 022 212 | 1 TiB | 2^−4 398 046 511 104 ≈ 10^(−1.32·10^12) | ≈ 2^4 398 046 511 104 ≈ 10^(1.32·10^12) | 5 | 4 |
| 43 | 17 592 186 044 420 | 2 TiB | 2^−8 796 093 022 208 ≈ 10^(−2.65·10^12) | ≈ 2^8 796 093 022 208 ≈ 10^(2.65·10^12) | 5 | 4 |
| 44 | 35 184 372 088 836 | 4 TiB | 2^−17 592 186 044 416 ≈ 10^(−5.30·10^12) | ≈ 2^17 592 186 044 416 ≈ 10^(5.30·10^12) | 5 | 4 |
| 45 | 70 368 744 177 668 | 8 TiB | 2^−35 184 372 088 832 ≈ 10^(−1.06·10^13) | ≈ 2^35 184 372 088 832 ≈ 10^(1.06·10^13) | 5 | 4 |
| 46 | 140 737 488 355 332 | 16 TiB | 2^−70 368 744 177 664 ≈ 10^(−2.12·10^13) | ≈ 2^70 368 744 177 664 ≈ 10^(2.12·10^13) | 5 | 4 |
| 47 | 281 474 976 710 660 | 32 TiB | 2^−140 737 488 355 328 ≈ 10^(−4.24·10^13) | ≈ 2^140 737 488 355 328 ≈ 10^(4.24·10^13) | 5 | 4 |
| 48 | 562 949 953 421 316 | 64 TiB | 2^−281 474 976 710 656 ≈ 10^(−8.47·10^13) | ≈ 2^281 474 976 710 656 ≈ 10^(8.47·10^13) | 5 | 4 |
| 49 | 1 125 899 906 842 628 | 128 TiB | 2^−562 949 953 421 312 ≈ 10^(−1.69·10^14) | ≈ 2^562 949 953 421 312 ≈ 10^(1.69·10^14) | 5 | 4 |
| 50 | 2 251 799 813 685 252 | 256 TiB | 2^−1 125 899 906 842 624 ≈ 10^(−3.39·10^14) | ≈ 2^1 125 899 906 842 624 ≈ 10^(3.39·10^14) | 5 | 4 |

¹ Actual allocation: mantissa limbs + one flags word =
  `8·(⌈2^(N+1)/64⌉ + 1)` bytes (64-bit aligned; `Vec` bookkeeping ~24 B not
  counted). An **NC** complex value is exactly **2×** this.
² Smallest positive *normal* = `2^−2^N`; any non-zero result below it
  becomes `±InfSmall`.
³ Largest normal = `2^2^N − 2^−2^N`; anything above becomes `±Inf`.
⁴ Tetration: `tet(b,1) = b`, `tet(b,h) = b^tet(b,h−1)` — so
  `tet(2,4) = 2^(2^(2^2)) = 65536`, `tet(e,3) = e^(e^e) ≈ 3 814 279.1.`
  The column is the largest `h` such that `tet(b,h)` ≤ largest value.
  Height `0` at N=0 means even the base itself does not fit (max 1.5).
  Notable near-misses: at N=4, `tet(2,4) = 2^16` overshoots the max by just
  `2^−16` (height stays 3); at N=16, `tet(2,5) = 2^65536` misses by `2^−65536`
  (height stays 4 — N=17 stores it); at N=2, `e^e ≈ 15.154` *just fits*
  under 15.9375. Next milestones are astronomically far: `tet(2,6)` first
  fits at **N = 65 537**, and `tet(e,5)` needs **N ≈ 5.5·10^6**
  (a ~10^1 656 520-bit number). Base-2 height plateaus at 5 for all
  17 ≤ N ≤ 65 536; base-e at 4 for all 23 ≤ N ≲ 5.5·10^6.

**There is no upper limit on `N`** other than available memory. Nothing is
checked or capped — if you ask for more than the machine has, the OS's OOM
killer is the limit.

The flag bits alone encode the special values **NaN**, **+Inf**, **−Inf**,
**+InfSmall**, **−InfSmall**, **+Any**, **−Any**. When the first three flags
are all zero the number is a normal value:
`(−1)^sign · mantissa / 2^(2^N)`.

### Special-value semantics (limit interpretation)

`Inf` means *"some value too large to represent"* (possibly a true infinity,
e.g. from `x/Eps`) and `InfSmall` (`Eps`) means *"some non-zero value too
small to represent"* (possibly a true infinitesimal). All operations treat
them as sign-tagged limits. Any normal result that overflows becomes `±Inf`;
any non-zero result that rounds to zero becomes `±InfSmall`. Rounding is
always round-to-nearest, ties away from zero. Zero is unsigned (`-0`
canonicalizes to `+0`).

### The `Any` state

What is `+Inf * +InfSmall`? The first factor is *some huge positive thing*,
the second *some tiny positive thing*: the product can be anything from
infinitesimally small, through any normal number, up to infinity — **but it
is certainly positive and certainly not zero**. Collapsing that to `NaN`
would throw the sign information away. NF therefore encodes it by setting
**both** the Inf and the InfSmall bits (NaN bit clear, sign bit meaningful):

```text
+Any = flags 0110 = "some value in (0, +Inf]"     (prints as +Any)
-Any = flags 0111 = "some value in [-Inf, 0)"     (prints as -Any)
```

The doctrine used throughout the library:

* an indeterminate form whose **sign is determined** yields `±Any`
  (`Inf*Eps`, `Inf/Inf`, `Eps/Eps`, `Inf^Eps`, `Eps^Eps`, ...);
* an indeterminate form whose **sign is unknown** stays `NaN`
  (`Inf-Inf`, `Eps-Eps`, `0*Inf`, `0*Any`, `x/0`, `1^Inf`, `1^Any`, ...);
* `Any` **absorbs**: adding/multiplying/dividing it by anything of known
  compatible sign stays `Any`; mixing it with an opposite-sign additive term
  makes the sign unknown → `NaN`.
* `Any` never equals anything (like NaN: `eq(Any, Any) = false`), but it
  **does** compare against values on the other side of zero:
  `-1 < +Any`, `+Any > 0` are decisive; `+Any vs +1` is undecidable.

Comparison is **strict** about all limit values: `±Inf`, `±InfSmall` and
`±Any` never equal anything — *including themselves*: `cmp(+Inf, +Inf)`,
`cmp(+Eps, +Eps)` and `cmp(+Any, +Any)` are all `?` (unordered). Two
overflows need not hide the same true value (see the Comparison matrix
below).

`Any` round-trips through strings (`any`, `-any` parse; prints `+Any`) and
through bit strings (`0110-...-...`). All special values parse
case-insensitively: `nan`, `inf`/`-inf`, `eps`/`infsmall` (± forms), `any`/
`-any`, and `zero`/`-zero` (aliases for `0`/`-0`; zero is unsigned, so all
four parse to the same `0`).

## Operation matrices (all special cases, validated by tests)

Legend: `X` = NaN, `+∞/−∞` = ±Inf, `+A/−A` = ±Any, `+ε/−ε` = ±InfSmall,
`0` = zero, `+x/−x` = positive/negative normal (shown for `±1`).
Every cell below is asserted verbatim by `tests/matrices.rs` for every
`N ∈ {0, 1, 2, 3, 6, 7, 8}` (10×10 grids = 100 cells per operation per N).

### Addition (`a + b`, rows = `a`, columns = `b`)

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | +x | −x |
|-----|---|----|----|----|----|----|----|---|----|----|
| **X**  | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +∞ | X | +∞ | X | +∞ | +∞ | +∞ | +∞ | +∞ |
| **−∞** | X | X | −∞ | X | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ |
| **+A** | X | +∞ | X | +A | X | +A | X | +A | +A | X |
| **−A** | X | X | −∞ | X | −A | X | −A | −A | X | −A |
| **+ε** | X | +∞ | −∞ | +A | X | +ε | X | +ε | +x | −x |
| **−ε** | X | +∞ | −∞ | X | −A | X | −ε | −ε | +x | −x |
| **0**  | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | +x | −x |
| **+x** | X | +∞ | −∞ | +A | X | +x | +x | +x | sum | sum |
| **−x** | X | +∞ | −∞ | X | −A | −x | −x | −x | sum | sum |

Notes: same-sign `∞+∞ = ∞`, opposite signs → `X`. `ε` is absorbed by any
normal (`1+ε = 1`). `+A` plus anything non-negative stays `+A` (it may be
huge); plus anything negative (even `−ε`) the sign becomes unknown → `X`.
`sum` = normal addition; overflow → `±∞`, a non-zero sum rounding to 0 → `±ε`.

### Subtraction (`a − b`)

`a − b ≡ a + (−b)` exactly — negation just flips the sign bit of any state
(`−(+A) = −A`, `−(+ε) = −ε`, `−0 = 0`). The test suite checks all 100 state
pairs against the addition matrix under this identity. Consequences:
`∞−∞ = X`, `ε−ε = X`, `A−A = X`, `+A−(−A) = +A`, `0−(+A) = −A`.

### Multiplication (`a · b`; sign = XOR of signs)

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | +x | −x |
|-----|---|----|----|----|----|----|----|---|----|----|
| **X**  | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +∞ | −∞ | +A | −A | **+A** | **−A** | X | +∞ | −∞ |
| **−∞** | X | −∞ | +∞ | −A | +A | **−A** | **+A** | X | −∞ | +∞ |
| **+A** | X | +A | −A | +A | −A | +A | −A | X | +A | −A |
| **−A** | X | −A | +A | −A | +A | −A | +A | X | −A | +A |
| **+ε** | X | **+A** | **−A** | +A | −A | +ε | −ε | 0 | +ε | −ε |
| **−ε** | X | **−A** | **+A** | −A | +A | −ε | +ε | 0 | −ε | +ε |
| **0**  | X | X | X | X | X | 0 | 0 | 0 | 0 | 0 |
| **+x** | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | prod | prod |
| **−x** | X | −∞ | +∞ | −A | +A | −ε | +ε | 0 | prod | prod |

The bold cells are the signature rule: **`Inf · Eps = Any`** with the XOR
sign — not NaN, because the sign is known even though the magnitude is not.
`0 · Inf` and `0 · Any` stay `X` (the operand may be a true infinity), while
`0 · ε = 0` (an infinitesimal times exact zero is exactly zero).

### Division (`a / b`; sign = XOR of signs)

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | +x | −x |
|-----|---|----|----|----|----|----|----|---|----|----|
| **X**  | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | **+A** | **−A** | +A | −A | +∞ | −∞ | X | +∞ | −∞ |
| **−∞** | X | **−A** | **+A** | −A | +A | −∞ | +∞ | X | −∞ | +∞ |
| **+A** | X | +A | −A | +A | −A | +A | −A | X | +A | −A |
| **−A** | X | −A | +A | −A | +A | −A | +A | X | −A | +A |
| **+ε** | X | +ε | −ε | +A | −A | **+A** | **−A** | X | +ε | −ε |
| **−ε** | X | −ε | +ε | −A | +A | **−A** | **+A** | X | −ε | +ε |
| **0**  | X | 0 | 0 | 0 | 0 | 0 | 0 | X | 0 | 0 |
| **+x** | X | +ε | −ε | +A | −A | +∞ | −∞ | X | quot | quot |
| **−x** | X | −ε | +ε | −A | +A | −∞ | +∞ | X | quot | quot |

`Inf/Inf` and `Eps/Eps` are `±Any` (two unknown magnitudes of the same
class — the ratio can be anything, but its sign is the XOR). Division by
exact `0` is always `X` (zero is unsigned, so the side cannot be chosen);
`0/b = 0` for every non-zero, non-NaN `b` including `±Any`.

### Exponentiation (`a ^ b`, rows = base `a`, columns = exponent `b`)

This grid is asserted cell-by-cell (bases × exponents shown; numeric cells
are checked where the literal is representable at the given `N`):

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 2 | −2 | 3 | 0.5 |
|------|---|----|----|----|----|----|----|---|---|----|---|-----|
| **X**    | X | X | X | X | X | X | X | **X** | X | X | X | X |
| **+∞**   | X | +∞ | +ε | +∞ | +A | **+A** | **+A** | 1 | +∞ | +ε | +∞ | +∞ |
| **−∞**   | X | X | X | X | X | X | X | 1 | +∞ | +ε | −∞ | X |
| **+A**   | X | +A | +A | +A | +A | +A | +A | 1 | +A | +A | +A | +A |
| **−A**   | X | X | X | X | X | X | X | 1 | +A | +A | −A | X |
| **+ε**   | X | +ε | +∞ | +A | +A | **+A** | **+A** | 1 | +ε | +∞ | +ε | +ε |
| **−ε**   | X | X | X | X | X | X | X | 1 | +ε | +∞ | −ε | X |
| **0**    | X | 0 | X | 0 | X | 0 | X | 1 | 0 | X | 0 | 0 |
| **1**    | X | X | X | X | X | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| **−1**   | X | X | X | X | X | X | X | 1 | 1 | 1 | −1 | X |
| **2**    | X | +∞ | +ε | +A | +A | 1 | 1 | 1 | 4 | 0.25 | 8 | √2 |
| **−2**   | X | X | X | X | X | X | X | 1 | 4 | 0.25 | −8 | X |
| **0.5**  | X | +ε | +∞ | +A | +A | 1 | 1 | 1 | 0.25 | 4 | 0.125 | √0.5 |
| **−0.5** | X | X | X | X | X | X | X | 1 | 0.25 | 4 | −0.125 | X |

Highlights:

* `x^0 = 1` for **every** non-NaN `x` (`0^0 = 1`, `∞^0 = 1`, `Any^0 = 1`);
  `NaN^0 = NaN` (NaN poisons first — a deliberate deviation from IEEE).
* `Inf^±Eps = Eps^±Eps = +Any`: this is `e^(±Eps·Inf) = e^(±Any)` — a
  positive number of completely unknown size.
* `1^Inf = 1^Any = NaN` (classic indeterminate; the exponent may be a true
  infinity), but `1^Eps = 1` (an infinitesimal exponent is finite).
* Negative bases require an exactly-integral exponent: sign follows parity
  (`(−A)^odd = −A`, `(−A)^even = +A`, `(−ε)^3 = −ε`, `(−∞)^2 = +∞`);
  any non-integral exponent (including all `ε/∞/A` exponents) → `X`.
* `+A^b = +A` for every non-zero non-NaN `b` — a positive unknown stays a
  positive unknown (its magnitude class simply cannot be pinned down).
* `0^negative = X` (division by zero in disguise), `0^positive = 0`,
  including `0^+A = 0` and `0^−A = X`.

### Roots (`root(a, k)`, integral `k ≥ 0`)

| a \ k | 0 | 1 | 2 | 3 | 4 (any even) |
|-------|---|---|---|---|--------------|
| **X**  | X | X | X | X | X |
| **+∞** | X | +∞ | +∞ | +∞ | +∞ |
| **−∞** | X | −∞ | X | −∞ | X |
| **+A** | X | +A | +A | +A | +A |
| **−A** | X | −A | X | −A | X |
| **+ε** | X | +ε | +ε | +ε | +ε |
| **−ε** | X | −ε | X | −ε | X |
| **0**  | X | 0 | 0 | 0 | 0 |
| **+x** | X | +x | √x | ∛x | ⁴√x |
| **−x** | X | −x | X | ∛(−x) | X |

Even roots of anything negative are `X`; odd roots preserve sign for every
state. `root(a, 0)` is `X`. In the calculator `root(z, k)` with a
non-integral or special `k` falls back to `z^(1/k)`, so e.g.
`root(2, inf) = 2^(1/∞) = 2^ε = 1`, `root(2, eps) = 2^∞ = +Inf`,
`root(2, any) = 2^(1/A) = 2^A = +Any`, `root(2, 0) = 2^(1/0) = X`.

### Square (`a²` = `a·a` = `a^2` — all three agree, tested)

| X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | ±x |
|---|----|----|----|----|----|----|---|----|
| X | +∞ | +∞ | +A | +A | +ε | +ε | 0 | x² |

and `sqrt(a²)` = `|a|`-state: `sqrt((±A)²) = +A`, `sqrt((±∞)²) = +∞`,
`sqrt((±ε)²) = +ε`.

### Comparison (`cmp(a, b)`: `<`, `=`, `>`, or `?` = unordered)

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | +x | −x |
|-----|---|----|----|----|----|----|----|---|----|----|
| **X**  | ? | ? | ? | ? | ? | ? | ? | ? | ? | ? |
| **+∞** | ? | ? | > | ? | > | > | > | > | > | > |
| **−∞** | ? | < | ? | < | ? | < | < | < | < | < |
| **+A** | ? | ? | > | ? | > | ? | > | > | ? | > |
| **−A** | ? | < | ? | < | ? | < | ? | < | < | ? |
| **+ε** | ? | < | > | ? | > | ? | > | > | < | > |
| **−ε** | ? | < | > | < | ? | < | ? | < | < | > |
| **0**  | ? | < | > | < | > | < | > | = | < | > |
| **+x** | ? | < | > | ? | > | > | > | > | cmp | > |
| **−x** | ? | < | > | < | ? | < | < | < | < | cmp |

Only **zero and normal values can ever be `=`** — they are the only states
that denote one exact number. Everything else is a *set* of possible true
values, and two sets never compare equal: `+Inf` means "some value >
max", so `cmp(+Inf, +Inf)` is `?` (the two hidden values could differ —
think ℵ₀ vs ℵ₁; the same holds for two underflows in `(0, 2^−2^N)`). This
is the exact comparison analogue of `Inf − Inf = NaN` and `Inf / Inf =
Any`. Order against *disjoint* ranges is still decisive: `+Inf > x`,
`+ε < x`, `+ε > 0`, `−∞ < −x` — every `<`/`>` cell above is a strict
statement about disjoint intervals.

`±Any` is ordered only against values strictly on the other side of zero
(and against zero itself); against anything on its own side the answer is
`?`. `eq` is `cmp == Equal`, so `NaN`, `±Inf`, `±InfSmall` and `±Any` are
never equal to anything, including themselves.

### Unary functions over every special state

All rows verified for every `N ∈ {0,1,2,3,6,7,8}`:

| f(a)      | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 |
|-----------|---|----|----|----|----|----|----|---|
| `neg`     | X | −∞ | +∞ | −A | +A | −ε | +ε | 0 |
| `abs`     | X | +∞ | +∞ | +A | +A | +ε | +ε | 0 |
| `square`  | X | +∞ | +∞ | +A | +A | +ε | +ε | 0 |
| `sqrt`    | X | +∞ | X  | +A | X  | +ε | X  | 0 |
| `exp`     | X | +∞ | +ε | +A | +A | 1  | 1  | 1 |
| `ln`      | X | +∞ | X  | X  | X  | −∞ | X  | −∞ |
| `log2`    | X | +∞ | X  | X  | X  | −∞ | X  | −∞ |
| `log10`   | X | +∞ | X  | X  | X  | −∞ | X  | −∞ |
| `floor`   | X | +∞ | −∞ | X  | −A | 0  | −1 | 0 |
| `ceil`    | X | +∞ | −∞ | +A | X  | 1  | 0  | 0 |
| `round`   | X | +∞ | −∞ | X  | X  | 0  | 0  | 0 |
| `trunc`   | X | +∞ | −∞ | X  | X  | 0  | 0  | 0 |
| `sin`     | X | X  | X  | X  | X  | +ε | −ε | 0 |
| `cos`     | X | X  | X  | X  | X  | 1  | 1  | 1 |
| `tan`     | X | X  | X  | X  | X  | +ε | −ε | 0 |
| `asin`    | X | X  | X  | X  | X  | +ε | −ε | 0 |
| `atan`    | X | π/2 | −π/2 | +A | −A | +ε | −ε | 0 |
| `sinh`    | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 |
| `cosh`    | X | +∞ | +∞ | +A | +A | 1  | 1  | 1 |
| `tanh`    | X | 1  | −1 | +A | −A | +ε | −ε | 0 |
| `asinh`   | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 |
| `acosh`   | X | +∞ | X  | X  | X  | X  | X  | X |
| `atanh`   | X | X  | X  | X  | X  | +ε | −ε | 0 |

Reasoning for the `Any` column: `exp(±A) = +A` (always positive, unknown
size); `ln(±A) = X` (the argument may be above or below 1, so even the sign
of the result is unknown); `atan/tanh/sinh/asinh` are odd, sign-preserving,
zero-free on `(0,∞]` → `±A`; `cosh ≥ 1` → `+A`; periodic functions → `X`;
`floor(+A) = X` (could be 0 or huge — 0 crosses to the wrong side) but
`floor(−A) = −A` (stays negative), and symmetrically for `ceil`;
`acosh/atanh` have bounded domains the unknown magnitude may violate → `X`.

`atan2(y, x)` with `Any`: `atan2(0, +A) = 0`, `atan2(0, −A) = π`, otherwise
any `Any` in either argument yields `±Any` with the sign of `y`
(the angle's magnitude is unknown but its half-plane is known).
Other special cases: `atan2(0,0) = X`, `atan2(±∞, ∞) = X`,
`atan2(±ε, ±ε) = X`, `atan2(±1, +∞) = ±ε`, `atan2(±ε, −x) = ±π`.

### The complete 0F universe: four 14×14 grids

At `N = 0` the format is only 6 bits, so it holds **exactly 14 distinct
canonical values**: the seven specials `NaN`, `±Inf`, `±Any`, `±InfSmall`,
zero, and *all six* non-zero normals `±0.5`, `±1`, `±1.5`. The grids below
therefore show **every possible value against every possible value** for
`add`, `mul`, `div` and `pow` — 4 × 196 cells, the entire 0F arithmetic.

Every cell is **computed output of the library itself**, regenerated
verbatim by `cargo run --release --example readme_0f_matrices`, and the
same results are independently bit-exactly verified by `make test ALL0F=1`
(all 64×64 raw bit patterns per operation against an oracle). Same legend
as above (`X` = NaN, `±∞`, `±A` = ±Any, `±ε` = ±InfSmall).

Things to notice: at this tiny precision overflow is everywhere
(`1.5 + 0.5 = 2 → +∞`, `1.5 · 1.5 = 2.25 → +∞`); rounding is to nearest,
ties away from zero (`0.5 · 0.5 = 0.25 → 0.5`); `1^±∞ = X` but
`1^±ε = 1`; `0^0 = 1`; `+∞^−1 = +ε`; negative bases with non-integer
exponents are `X`; and every `±Any` row/column follows the sign algebra.

### Addition (`a + b`) — complete 0F grid

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
|-----|----|----|----|----|----|----|----|----|----|----|----|----|----|----|
| **X** | X | X | X | X | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +∞ | X | +∞ | X | +∞ | +∞ | +∞ | +∞ | +∞ | +∞ | +∞ | +∞ | +∞ |
| **−∞** | X | X | −∞ | X | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ | −∞ |
| **+A** | X | +∞ | X | +A | X | +A | X | +A | +A | X | +A | X | +A | X |
| **−A** | X | X | −∞ | X | −A | X | −A | −A | X | −A | X | −A | X | −A |
| **+ε** | X | +∞ | −∞ | +A | X | +ε | X | +ε | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
| **−ε** | X | +∞ | −∞ | X | −A | X | −ε | −ε | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
| **0** | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
| **0.5** | X | +∞ | −∞ | +A | X | 0.5 | 0.5 | 0.5 | 1 | 0 | 1.5 | −0.5 | +∞ | −1 |
| **−0.5** | X | +∞ | −∞ | X | −A | −0.5 | −0.5 | −0.5 | 0 | −1 | 0.5 | −1.5 | 1 | −∞ |
| **1** | X | +∞ | −∞ | +A | X | 1 | 1 | 1 | 1.5 | 0.5 | +∞ | 0 | +∞ | −0.5 |
| **−1** | X | +∞ | −∞ | X | −A | −1 | −1 | −1 | −0.5 | −1.5 | 0 | −∞ | 0.5 | −∞ |
| **1.5** | X | +∞ | −∞ | +A | X | 1.5 | 1.5 | 1.5 | +∞ | 1 | +∞ | 0.5 | +∞ | 0 |
| **−1.5** | X | +∞ | −∞ | X | −A | −1.5 | −1.5 | −1.5 | −1 | −∞ | −0.5 | −∞ | 0 | −∞ |

### Multiplication (`a * b`) — complete 0F grid

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
|-----|----|----|----|----|----|----|----|----|----|----|----|----|----|----|
| **X** | X | X | X | X | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +∞ | −∞ | +A | −A | +A | −A | X | +∞ | −∞ | +∞ | −∞ | +∞ | −∞ |
| **−∞** | X | −∞ | +∞ | −A | +A | −A | +A | X | −∞ | +∞ | −∞ | +∞ | −∞ | +∞ |
| **+A** | X | +A | −A | +A | −A | +A | −A | X | +A | −A | +A | −A | +A | −A |
| **−A** | X | −A | +A | −A | +A | −A | +A | X | −A | +A | −A | +A | −A | +A |
| **+ε** | X | +A | −A | +A | −A | +ε | −ε | 0 | +ε | −ε | +ε | −ε | +ε | −ε |
| **−ε** | X | −A | +A | −A | +A | −ε | +ε | 0 | −ε | +ε | −ε | +ε | −ε | +ε |
| **0** | X | X | X | X | X | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| **0.5** | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 0.5 | −0.5 | 1 | −1 |
| **−0.5** | X | −∞ | +∞ | −A | +A | −ε | +ε | 0 | −0.5 | 0.5 | −0.5 | 0.5 | −1 | 1 |
| **1** | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
| **−1** | X | −∞ | +∞ | −A | +A | −ε | +ε | 0 | −0.5 | 0.5 | −1 | 1 | −1.5 | 1.5 |
| **1.5** | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 1 | −1 | 1.5 | −1.5 | +∞ | −∞ |
| **−1.5** | X | −∞ | +∞ | −A | +A | −ε | +ε | 0 | −1 | 1 | −1.5 | 1.5 | −∞ | +∞ |

### Division (`a / b`) — complete 0F grid

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
|-----|----|----|----|----|----|----|----|----|----|----|----|----|----|----|
| **X** | X | X | X | X | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +A | −A | +A | −A | +∞ | −∞ | X | +∞ | −∞ | +∞ | −∞ | +∞ | −∞ |
| **−∞** | X | −A | +A | −A | +A | −∞ | +∞ | X | −∞ | +∞ | −∞ | +∞ | −∞ | +∞ |
| **+A** | X | +A | −A | +A | −A | +A | −A | X | +A | −A | +A | −A | +A | −A |
| **−A** | X | −A | +A | −A | +A | −A | +A | X | −A | +A | −A | +A | −A | +A |
| **+ε** | X | +ε | −ε | +A | −A | +A | −A | X | +ε | −ε | +ε | −ε | +ε | −ε |
| **−ε** | X | −ε | +ε | −A | +A | −A | +A | X | −ε | +ε | −ε | +ε | −ε | +ε |
| **0** | X | 0 | 0 | 0 | 0 | 0 | 0 | X | 0 | 0 | 0 | 0 | 0 | 0 |
| **0.5** | X | +ε | −ε | +A | −A | +∞ | −∞ | X | 1 | −1 | 0.5 | −0.5 | 0.5 | −0.5 |
| **−0.5** | X | −ε | +ε | −A | +A | −∞ | +∞ | X | −1 | 1 | −0.5 | 0.5 | −0.5 | 0.5 |
| **1** | X | +ε | −ε | +A | −A | +∞ | −∞ | X | +∞ | −∞ | 1 | −1 | 0.5 | −0.5 |
| **−1** | X | −ε | +ε | −A | +A | −∞ | +∞ | X | −∞ | +∞ | −1 | 1 | −0.5 | 0.5 |
| **1.5** | X | +ε | −ε | +A | −A | +∞ | −∞ | X | +∞ | −∞ | 1.5 | −1.5 | 1 | −1 |
| **−1.5** | X | −ε | +ε | −A | +A | −∞ | +∞ | X | −∞ | +∞ | −1.5 | 1.5 | −1 | 1 |

### Power (`a ^ b`) — complete 0F grid

| a\b | X | +∞ | −∞ | +A | −A | +ε | −ε | 0 | 0.5 | −0.5 | 1 | −1 | 1.5 | −1.5 |
|-----|----|----|----|----|----|----|----|----|----|----|----|----|----|----|
| **X** | X | X | X | X | X | X | X | X | X | X | X | X | X | X |
| **+∞** | X | +∞ | +ε | +∞ | +A | +A | +A | 1 | +∞ | +ε | +∞ | +ε | +∞ | +ε |
| **−∞** | X | X | X | X | X | X | X | 1 | X | X | −∞ | −ε | X | X |
| **+A** | X | +A | +A | +A | +A | +A | +A | 1 | +A | +A | +A | +A | +A | +A |
| **−A** | X | X | X | X | X | X | X | 1 | X | X | −A | −A | X | X |
| **+ε** | X | +ε | +∞ | +A | +A | +A | +A | 1 | +ε | +∞ | +ε | +∞ | +ε | +∞ |
| **−ε** | X | X | X | X | X | X | X | 1 | X | X | −ε | −∞ | X | X |
| **0** | X | 0 | X | 0 | X | 0 | X | 1 | 0 | X | 0 | X | 0 | X |
| **0.5** | X | +ε | +∞ | +A | +A | 1 | 1 | 1 | 0.5 | 1.5 | 0.5 | +∞ | 0.5 | +∞ |
| **−0.5** | X | X | X | X | X | X | X | 1 | X | X | −0.5 | −∞ | X | X |
| **1** | X | X | X | X | X | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| **−1** | X | X | X | X | X | X | X | 1 | X | X | −1 | −1 | X | X |
| **1.5** | X | +∞ | +ε | +A | +A | 1 | 1 | 1 | 1 | 1 | 1.5 | 0.5 | +∞ | 0.5 |
| **−1.5** | X | X | X | X | X | X | X | 1 | X | X | −1.5 | −0.5 | X | X |

## Representation

Numbers are **raw machine-word arrays** (`Vec<u64>` / `&[u64]`) — no structs,
no metadata. `N` is set **once per process** (`nf::set_n(n)`); it is global
configuration, never stored in numbers and never passed to functions. Every
number in one invocation shares the same `N`.

Word layout of one NF number (`nf::words()` = `ceil(2^(N+1)/64) + 1` words):

```text
[ mantissa limb 0 (least significant), ..., mantissa limb L−1, FLAGS ]
```

The `FLAGS` word holds bits `1`=NaN, `2`=Inf, `4`=InfSmall, `8`=Sign.
Storage is word-aligned for speed; `to_bits`/`from_bits` produce the exact
packed `2·2^N+4`-bit form, and `to_bit_string` shows the spec's bit layout —
1st bit NaN, 2nd Inf, 3rd InfSmall, 4th Sign, then integer bits, then
fraction bits:

```console
$ ./target/release/nc 3 --bits -- "-2"
-2
re bits: 0001-00000010-00000000
$ ./target/release/nc 3 --from-bits "0001-00000010-00000000"
-2
bits: 0001-00000010-00000000
$ ./target/release/nc 1 --from-bits "0000-00-00"
0
bits: 0000-00-00
$ ./target/release/nc 0 -B "0000-1-1"      # N=0: the 6-bit format
1.5
bits: 0000-1-1
```

(flags `0001` = sign-negative, integer `00000010` = 2, fraction `00000000`
— i.e. −2.0, matching the format definition.) All 16 flag nibbles decode:
`1xxx` → NaN, `0100/0101` → ±Inf, `0010/0011` → ±InfSmall, `0110/0111` →
±Any, `0000/0001` → the payload value (and `-0` canonicalizes to `+0`; a
special's payload canonicalizes to zero).

An NC (complex) number is simply two NF arrays back to back:
`[re words..., im words...]`.

## The `nc` calculator

```text
nc [OPTIONS] N "EXPRESSION"

  N            precision parameter (e.g. 7 or 7F)
  -d, --digits D   print D fraction digits (default: shortest round-trip)
  -b, --bits       also print raw flag/integer/fraction bits per component
  -B, --from-bits  EXPRESSION is a raw bit string (e.g. 0001-00000010-00000000,
                   separators optional): prints its value and canonical bits
  -q, --quiet      compute but print nothing — for enormous N where decimal
                   printing is impractical; the exit code still reports the
                   outcome
  -h, --help       full help
```

* Operators: `+ − * / ^` (also `**`), parentheses, implicit multiplication
  (`2pi`, `(1+2i)(3-i)`, `3i`).
* Constants: `pi`, `tau`, `e`, `i`, `inf`, `eps` (aka `infsmall`), `any`,
  `nan`, `zero` (case-insensitive; `-zero` = `-0` = `0` — zero is
  unsigned).
* Functions: `re im abs arg conj sqrt cbrt root(z,k) exp ln log log2 log10
  logb(b,x) pow(a,b) atan2(y,x) sin cos tan asin acos atan sinh cosh tanh
  asinh acosh atanh floor ceil round trunc` — all over complex numbers
  (principal branches).
* Number literals: arbitrary length, optional `i` suffix, scientific
  notation (`1.5e-3`), `_` separators.

Exit codes: `0` result computed (NaN/Inf are results), `1` usage error,
`2` expression error.

```console
$ nc 10 "exp(i*pi)"
-1-(+InfSmall)i            # Euler: the imaginary dust is provably non-zero but < 1 ulp
$ nc 8 -d 50 "sqrt(2)"
1.41421356237309504880168872420969807856967187537695
$ nc 7 "1/0"
(NaN)
$ nc 7 "inf*eps"
(+Any)                     # sign known, magnitude unknown — see the matrices
$ nc 4 "1000000"
(+Inf)                     # overflow at N=4 (max ≈ 65536)
$ nc 7 "(1+2i)(3-i)"
5+5i
```

## Library use

```rust
use nf::{nf as nff, nc, real, expr};

nff::set_n(7);                                  // once per process

let a = nff::from_str("3.25").unwrap();         // Vec<u64>
let b = nff::from_str("-0.125").unwrap();
let s = nff::add(&a, &b);
assert_eq!(nff::to_string(&s), "3.125");

let r = real::sqrt(&nff::from_u64(2));          // transcendentals on NF
let z = nc::make(&a, &b);                       // complex 3.25−0.125i
let w = nc::exp(&z);

let v = expr::eval_str("(1+i)^8").unwrap();     // full parser
assert_eq!(nc::to_string(&v), "16");
```

Modules:

| module | contents |
|--------|----------|
| `limb` | big-integer core on `u64` words: add/sub/shift, schoolbook + Karatsuba multiply, Knuth-D division, integer sqrt/k-th root, powers of 10 |
| `fx`   | signed fixed-point working type with caller-managed scale |
| `nf`   | the NF format: global `N`, packing/rounding/flags, arithmetic, comparisons, floor/ceil/round/trunc, decimal + bit-string I/O |
| `real` | correctly-computed transcendentals on NF: sqrt, n-th root, exp, ln, log2/log10/logb, pow, sin/cos/tan(+inverses), sinh/cosh/tanh(+inverses), atan2, cached π/e/ln 2 |
| `nc`   | complex numbers: exact single-rounding mul/div/abs, arg, exp/ln/sqrt/pow/root, trig/hyperbolic + inverses (principal branches), formatting |
| `expr` | tokenizer + recursive-descent parser + evaluator |

### Accuracy & performance

* `+ − × ÷` and `sqrt` round the exact result once (≤ 0.5 ulp + directed
  tie); complex `×`, `÷`, `abs` are computed exactly at mantissa level and
  rounded once per component.
* Transcendentals evaluate with 128 guard bits and argument reduction
  (`exp`: 2^k·ln 2 splitting; `ln`: 16 square-root reductions + atanh
  series; trig: π/2 quadrant reduction; constants by Machin-type formulas),
  typically correct to within a few final-bit ulps.
* Multiplication uses Karatsuba above 32 limbs; division is Knuth
  Algorithm D; integer roots use seeded Newton iteration.
* Sparse values stay fast at *any* precision: multiplication trims leading
  zeros, division strips common trailing zero limbs (`u·B^t / v·B^t` has
  the same quotient), `pow10` uses binary exponentiation, and
  shortest-form printing caps its search at the value's exact decimal
  length (`F − trailing zero bits` fraction digits). This is what makes
  the huge-`N` tests below practical: at `N=18`, `1/3` or `2^100000` are
  instant, while *dense*-mantissa transcendentals (Newton roots of normal
  values, π/e series) remain inherently quadratic in the width.
* Typical timings (one core): `N=10` (2052-bit) full example — ~10 ms;
  `N=16` (128K-bit) arithmetic — ~10 ms; `N=18` `sqrt(2)` — ~1.4 s;
  `N=20` (2M-bit) `sqrt` — ~15 s.

## How it works internally

The library is a strict five-layer stack; each layer only uses the one
below it:

```text
expr   tokenizer → recursive-descent parser → NC evaluator
nc     complex pairs of NF; exact cross-sums or structural special algebra
real   transcendentals: NF → Fx (+128 guard bits) → series → one rounding
nf     the format: global N, flags, canonical packing, ± × ÷, compare, I/O
fx     signed fixed-point scratch values at a caller-chosen scale
limb   unsigned big integers on u64 words (add/sub/mul/div/shift/sqrt)
```

**One global `N`, plain arrays.** `set_n(N)` stores `N` in a process-wide
atomic; every size is derived from it (`F = frac_bits() = 2^N`, mantissa =
`2F` bits, `words() = ceil(2F/64) + 1`). Numbers are raw `Vec<u64>` with no
header, so a value is just `[mantissa limbs…, FLAGS]` and slicing an NC
into its two NF halves is free. Tests that need several precisions use
`with_n(n, …)`, which takes a mutex around the atomic.

**Anatomy of every operation.** Each arithmetic function has the same
shape, and this is the core invariant of the whole library:

1. *Special ladder first* — NaN poisons; then the documented matrices
   decide `±Inf`/`±Any`/`±InfSmall`/zero cases (e.g. `0×Inf = NaN`,
   `Inf×Eps = +Any`) before any mantissa math runs.
2. *Exact integer arithmetic wider than the target* — a value is
   `mant/2^F` in sign-magnitude, so `+ −` are exact; `×` forms the full
   `4F`-bit product; `÷` computes `(mant_a << (F+1)) / mant_b`, one extra
   bit beyond the target.
3. *Exactly one rounding* — `limb::round_shr` shifts the exact result
   down (`F` bits after `×`, the single extra bit after `÷`), adding 1
   when the highest dropped bit is set: round-to-nearest, **ties away
   from zero**, ≤ 0.5 ulp total error.
4. *Canonical packing* — `nf::pack(sign, mant, nonzero_hint)` maps the
   rounded magnitude into the format: more than `2F` bits → `±Inf`
   (overflow bit), a zero magnitude whose true value was non-zero →
   `±InfSmall` (underflow bit), `−0` → `+0`, and specials always carry a
   zero payload. Every constructor and parser funnels through the same
   canonicalization, so operations only ever see canonical values.

**The special algebra** is interval reasoning made closed: `±Any` (Inf and
InfSmall bits set together) means *sign known, magnitude unknown in
(0, ∞]* — the answer whenever a result is provably non-zero of known sign
but its size cannot be bounded (`Inf/Inf`, `Inf×Eps`, `Eps/Eps`). NaN
appears exactly when even the sign is unknowable (`Inf−Inf`, `0×Inf`,
`x/0`). The README matrices above are the specification; the code's
ladders implement them case by case and the test suites assert every cell.

**Transcendentals (`real`)** convert the NF argument into an `Fx` — a
signed fixed-point scratch value at scale `F + 128` guard bits — run
argument reduction plus a series at that scale, and round **once** at the
end through the same `pack` path (`from_fx`): `exp` splits off `k·ln 2` so
the remainder series converges fast; `ln` applies 16 square roots and an
`atanh` series; `sin`/`cos` reduce to a quarter period; π, e, ln 2 come
from Machin-type formulas and are cached per (constant, scale) behind a
mutex. Special arguments short-circuit before any series, following the
unary tables above (`exp(−∞) = +ε`, `ln(+ε) = −∞`, `cos(±ε) = 1`, …).

**`pow` takes three routes**: the special-value table; exact binary
powering for integral exponents; and `exp(b·ln a)` for fractional ones.
The approximate route has a safety net — if the guarded approximation
lands within a small band of a rounding boundary (an odd multiple of
`2^−(F+1)`), the true side is decided by *exact integer* comparison
(`a^b ⋛ M/2^(F+1)` cross-multiplied into integers for `b = ±k/2^d`), so
genuine ties like `2.25^1.5 = 3.375` round away from zero instead of
falling to floating error. The exhaustive 1F sweep is what forced this
fix; the 2F sweep re-proves it at `F = 16` (e.g. `0.5^5 = 1/32`, an exact
tie).

**Complex (`nc`)** stores `[re NF | im NF]` and keeps the single-rounding
discipline: for all-normal operands `×` forms the exact cross sums
`ac − bd`, `ad + bc` at scale `2F` and rounds each component once, `÷`
computes the exact big rationals `(ac+bd)/(c²+d²)`, `(bc−ad)/(c²+d²)`
with one tie bit, and `abs` is a rounded integer square root of the exact
`ma² + mi²`. When any component is special, the code switches to
*structural composition*: products with an exactly-zero factor vanish
structurally (a zero component never manufactures `0×Inf`), division uses
Smith's algorithm on NF components, and `pow` walks a ladder — real
base/exponent shortcut → exact binary powering for integral real
exponents → `exp(w·ln z)` — so the NF special algebra decides every
corner. This is why the exhaustive 0C sweep can predict all
16.7M×4 results from the scalar matrices alone.

**The parser (`expr`)** tokenizes, parses by recursive descent (`^`
right-associative, implicit multiplication like `2pi` or `(1+i)(1−i)`),
and evaluates directly into NC values, so the CLI is a thin wrapper:
parse arguments, `set_n`, evaluate, format.

The reason the format is this testable is the combination of *fixed
point* (no exponent interactions — rounding is a pure integer question)
with *single rounding* (the result is a mathematical function of the
inputs, not of an evaluation order): an independent oracle can predict
every output bit, which is exactly what the exhaustive suites below do.

## Tests

```console
$ make test          # = cargo test (the fixed-N suite below)
$ make test N=15     # the same suite, additionally run at precision N=15
$ make test ALL0F=1  # additionally: the exhaustive 0F suite (20 480 cases)
$ make test ALL1F=1  # additionally: the exhaustive 1F suite (327 680 cases)
$ make test ALL2F=1  # additionally: the exhaustive 2F suite (67 108 864 cases)
$ make test ALL0C=1  # additionally: the exhaustive 0C complex suite (67 108 864 cases)
```

The suite (**117 tests**) is deliberately exhaustive:

* **`tests/matrices.rs`** — the tables above, verbatim: 10×10 state grids
  for `add`, `mul`, `div` (plus `sub` checked as `add(a,−b)` over all 100
  pairs), the 14×12 `pow` grid, the 10×5 `root` grid, the 10×10 `cmp` grid,
  23 unary functions × 8 special states, `atan2` special cases, square
  consistency (`a·a ≡ a^2`, `sqrt(a²) = |a|`-state) — **each at every
  `N ∈ {0, 1, 2, 3, 6, 7, 8, 18}`** (N=0 is the 6-bit extreme where the
  largest value is 1.5; N=7's 260 bits exceed any native machine word;
  N=18 is the huge 524 292-bit case, ~64 KB per number); all 16 flag
  nibbles × {zero, non-zero} payloads decode and canonicalize at every N;
  bit-string round-trips for every state at every N with exact expected
  strings at N=0/1/3 (`0000-0-0`, `0000-00-00`, `0001-00000010-00000000`);
  `-0 → +0`; string round-trips for values and specials (`any` ↔ `+Any`);
  overflow/underflow boundary behavior (`max+min → +Inf`, `min² → +Eps`,
  tie-away artifacts at N=0 documented in-test); the whole special algebra
  again end-to-end through the parser (`inf*eps → (+Any)`, `0*any → (NaN)`,
  `root(2,eps) → (+Inf)`, ...), and plain numerics at every N.
  At N=18 the few paths that are inherently quadratic in the mantissa
  width (Newton roots of normal values, π-valued results, shortest-form
  printing of dense mantissas) are skipped — their logic is identical at
  every N and already proven at N ≤ 8.
* **`tests/huge.rs`** (7 tests) — N=18 stress: layout sizes; `2^1000`
  (302-digit) and `2^100000` (30 103-digit) exact decimal round-trips;
  exact sparse arithmetic spanning the full width
  (`a = 2^big + 2^-big` with `big = 3F/4 = 196608`: `(a+b)−b = a`,
  `(a·2)/2 = a`, exact floor/frac split); deep-fraction boundaries
  (`min = 2^-262144`: `min+min`, `min² → +Eps`, `1/min → +Inf`, `1−min`
  borrows across 4096 limbs); max-value boundaries (`max+min → +Inf`,
  `1/max = min`); 1101-char decimal literals round-tripping exactly
  (`(10^999+7) + 2^-100`); complex giants through the parser
  (`(2^100+3)(2^100−3) = 2^200−9` exactly).
* **library unit tests** (50) — big-int properties, packing/rounding,
  special algebra, transcendentals vs 80-digit references at N=7/8, complex
  identities (Euler, i^i, branch cuts), parser precedence/errors, across
  `N ∈ {1, 2, 3, 6, 7, 8}` with spot checks at N=0/10/12.
* **CLI integration tests** (13) — argument handling, exit codes, `--bits`,
  `--from-bits` (including malformed input → exit 2), `--quiet`, `±Any`
  output, N=18 end-to-end, and the spec's own examples.
* **`tests/allf.rs`** (10 tests, opt-in) — the exhaustive 0F and 1F checks.
  A 0F number is 6 bits and a 1F number is 8 bits, so *all* raw encodings
  can be enumerated — including every non-canonical one (NaN+Inf+InfSmall
  at once, specials with payload bits, `-0`). `make test ALL0F=1` feeds
  **all 64 × 64 = 4096 ordered pairs** and `make test ALL1F=1` **all
  256 × 256 = 65 536 ordered pairs** through each of `add`, `sub`, `mul`,
  `div`, `pow` (**20 480 + 327 680 = 348 160 cases**), and requires the
  full output encoding to match an independent exact oracle **bit for
  bit**. The oracle is implemented straight from the matrices above:
  small-integer rational arithmetic in units of 2^−F, round-to-nearest
  ties-away (pow decided by exact u128 power comparisons, no floating
  point), overflow → `±Inf`, underflow → `±InfSmall`. All 348 160 agree.
  Without the variables the ten tests are no-ops, keeping the default run
  fast.
* **The 1F sweep caught a real bug.** `2.25^1.5 = 3.375` lands *exactly*
  halfway between the two 1F neighbours 3.25 and 3.5, and the approximate
  `exp(b·ln a)` power path cannot decide an exact tie — it landed on the
  wrong side and returned 3.25 instead of the ties-away 3.5. `real::pow`
  now detects an approximation inside the error band around a rounding
  boundary (an odd multiple of 2^−(F+1)) and decides the true side with
  exact integer arithmetic (`a^b ⋛ M/2^(F+1)` ⇔
  `mant(a)^k·2^((F+1)·2^d) ⋛ M^(2^d)·2^(F·k)` for `b = ±k/2^d` in lowest
  terms), rounding a genuine tie away from zero. Guarded by the permanent
  unit test `pow_exact_tie_rounding` in the default suite.
* **`tests/all2f0c.rs`** (12 tests, the sweeps opt-in) — the same
  exhaustive idea pushed to 12-bit operands. `make test ALL2F=1` sweeps
  2F (units of 1/16, 4 096 raw encodings): **all 4096 × 4096 =
  16 777 216 ordered raw pairs through each of `add`, `mul`, `div`,
  `pow` = 67 108 864 bit-exact cases** (~5 s, threaded), plus all 4 096
  encodings against an independent canonicalizer. `make test ALL0C=1`
  does the same for **complex 0C** numbers (two 6-bit 0F components =
  12 bits): again all 16 777 216 raw pairs × the four `nc` ops,
  67 108 864 bit-exact cases (~2 s). `sub` is omitted by design — it is
  definitionally `add(a, −b)` and the negation matrix is covered — and
  NaN rows stay in the grid. To keep this fast, expected outputs are
  precomputed once per *canonical* pair (2F has 518 canonical values →
  268 324 pairs; 0C has 196 canonical complex values → 38 416 pairs) and
  the raw sweeps compare every library result against the cached
  encoding — sound because the raw → canonical parse mapping is itself
  verified exhaustively. Oracles stay independent of the library: the 2F
  `pow` oracle decides `a^b ⋛ (2m−1)/32` with exact integer comparisons
  on a self-contained bigint (values up to ~2^2120, no floating point;
  ties away — the grid contains genuine ties like `0.5^5 = 0.25^2.5 =
  1/32`, re-proving the midpoint fix at F=4), and the 0C oracle mirrors
  the documented complex semantics: exact once-rounded cross sums /
  big-rational quotients for all-normal operands, and for special
  operands the documented structural composition (term-wise NF algebra,
  Smith's division with the `cmp ≠ Less` branch, `pow` ladder down to
  `exp(w·ln z)` built from correctly-rounded 0F tables for
  `exp`/`ln`/`sin`/`cos`/`atan2`/`|z|` — at 0F, `π` itself overflows to
  `+Inf` and `cos 1.5` underflows to `+InfSmall`, and the sweep proves
  the library gets those right on all 134 217 728 cases).

### Testing at an arbitrary precision

`make test N=K` (equivalently `N=K make test`, env var `NF_TEST_N`) runs
the matrix grids, the whole of `tests/huge.rs`, and the huge CLI checks
*additionally* at precision `K` — on top of the fixed
`{0, 1, 2, 3, 6, 7, 8, 18}` set, which always runs.  Sub-checks that need
more integer bits than a small `K` provides (e.g. the 302-digit `2^1000`
round-trip, or values like 5 at the N=0/N=1 extremes where max is 1.5/3.75)
skip themselves, so any `K ≥ 0` works; at `K ≥ 16` the quadratic dense
paths are
skipped as in the fixed N=18 run.  Measured: `make test` ≈ 6 s,
`make test N=20` ≈ 44 s (debug, one core).

### Huge-N confirmation (N=32 and beyond)

Decimal printing makes no sense at N=32 (one NF is `2·2^32+4` =
8 589 934 596 logical bits = 1 GiB of mantissa; an NC is 2 GiB), so the
CLI gained `-q/--quiet` and the repo carries a permanent in-memory proof:

```console
$ make confirm32           # = cargo run --release --example confirm32 32
```

It fills **all data bits of two NC numbers with random bits** (xorshift64*,
time seed), adds them with `nc::add`, and spot-checks the exact
fixed-point sum limb-wise without printing a digit. Measured on this
machine:

```text
N=32  logical bits per NF: 2*(2^32)+4 = 8589934596  (134217728 limbs, 1.000 GiB/NF, 2.000 GiB/NC)
filled 4 random NF mantissas in 3.15s
nc::add done in 8.81s  ->  re: normal, im: normal
re: exact low-limb check passed
im: exact low-limb check passed
peak RSS: 8.00 GiB
OK: 32C addition confirmed in memory
```

No 32-bit limitation exists anywhere: widths are `usize`/`u64` throughout
(`frac_bits()` is `1usize << N`), so N is bounded only by RAM — exactly as
specified. `make confirm32 N=30` (or any other N) runs the same proof at
other sizes.

## Verified-working checklist

Every claim below is enforced by at least one test and reproduced here with
real output:

| claim | command | output |
|-------|---------|--------|
| parser + complex pow at N=7 | `nc 7 "(3.14159+2.73i)^(-2.0…51-0.0…01i)"` | `0.008053784635898781981668995528076383128-0.057163870131559166207370947318599216665i` |
| bit notation out | `nc 3 --bits -- "-2"` | `-2` + `re bits: 0001-00000010-00000000` |
| bit notation in | `nc 3 --from-bits "0001-00000010-00000000"` | `-2` + canonical bits |
| `0000-00-00` (N=1) works | `nc 1 -B "0000-00-00"` | `0` |
| 6-bit N=0 works | `nc 0 -B "0000-1-1"` | `1.5` |
| string I/O at any precision | `nc 8 -d 50 "sqrt(2)"` | 50 correct digits |
| complex math | `nc 10 "exp(i*pi)"` | `-1-(+InfSmall)i` |
| `Inf·Eps = +Any` (both bits set) | `nc 7 --bits "inf*eps"` | `(+Any)` + `re bits: 0110-0…0-0…0` |
| sign-known indeterminates | `nc 3 "inf/inf"`, `nc 3 "eps/eps"` | `(+Any)` |
| sign-unknown stay NaN | `nc 3 "inf-inf"`, `nc 3 "0*inf"` | `(NaN)` |
| strict comparison — `Inf` never equals `Inf` | `make test` (`matrices.rs` cmp grid) | `cmp(+∞,+∞) = ?`, `cmp(+ε,+ε) = ?` |
| `zero`/`-zero`/`Zero` parse | `nc 4 "zero"`, `nc 4 -- "-zero"` | `0` |
| the four 14×14 0F grids above are computed output | `cargo run --release --example readme_0f_matrices` | the tables, verbatim |
| huge N end-to-end | `nc 18 "2^100"` | `1267650600228229401496703205376` |
| every 12-bit 2F op result, bit-exact | `make test ALL2F=1` | 67 108 864 cases ok (~5 s) |
| every 12-bit 0C complex op result, bit-exact | `make test ALL0C=1` | 67 108 864 cases ok (~2 s) |
| quiet compute (no printing) | `nc 32 -q "…"` | *(nothing; exit 0)* |
| 32C in memory (8.6G-bit numbers) | `make confirm32` | `OK: 32C addition confirmed in memory` (8 GiB peak, 8.81 s) |

## Alternative storage layouts — an investigation (nothing here is implemented)

The current format spends **4 flag bits** on top of the `2F` payload bits
(`F = 2^N`). Is that wasteful? Information theory first: the format must
distinguish `2·(2^2F − 1) + 1` normal values (every non-zero magnitude in
two signs, plus one unsigned zero) **plus 7 specials** (`NaN`, `±Inf`,
`±InfSmall`, `±Any`) ≈ `2^(2F+1) + 6` states — so the hard floor is
**`2F + 2` bits**. The current `2F + 4` sits 2 bits above the floor. Three
candidate layouts:

### Scheme A′ — one "special" flag bit: `[special][sign][2F payload]` = `2F + 2` bits

If `special = 0`: sign + plain `2F`-bit magnitude (sign-magnitude, `-0`
canonicalized). If `special = 1`: the payload encodes which special value
it is — only 3 payload bits are needed for the 7 specials (the sign bit can
serve as the special's sign). This achieves the **information-theoretic
optimum**.

The original sketch (`[special][2F]`, `1F = 5 bits`) has a flaw: with
`special = 0` there is no sign bit left, so normals couldn't be negative.
The corrected A′ costs one more bit and works for every N — including
`0F`, which becomes **4 bits**: `[special][sign][i][f]`, and with
`special = 1` the remaining 3 bits are exactly enough for the 7 specials.
Sizes: 0F = 4, 1F = 6, 2F = 10, 3F = 18, 10F = 2050 bits.

Cost: every classification (`is_nan`, `is_inf`, …) becomes a *two-step*
test (flag, then decode payload), and `pack` must ensure a special's
payload is never interpreted as mantissa. Cheap, but no longer a 1-bit
probe.

### Scheme B — `[Inf][Eps][Sign]` + NaN as "−0": `2F + 3` bits

Drop the NaN bit; keep the other three. The freed pattern
"`sign = 1`, everything else zero" (i.e. **−0**, which the current format
canonicalizes away anyway) becomes NaN. `±Any` still works
(`Inf`+`Eps` both set). One bit saved.

Cost: `is_nan` changes from a 1-bit test to an **O(words) scan** of the
whole mantissa (it must verify all `2F` payload bits are zero with the
sign set) — on a 10F number that is a 32-limb scan on *every* NaN check,
and NaN is checked first in every single operation. A real throughput
regression for large N, to save one bit that alignment then swallows
anyway (see below).

### Scheme C — fully in-band ("saturated") encoding: exactly `2F` bits

No flag bits at all: steal the **top 7 codes** of the raw `2F`-bit space.
With an unsigned mantissa scale `m/2^F`, let `M = 2^2F − 1` (all ones):

```text
M   = NaN     M−1 = +Inf    M−2 = −Inf    M−3 = +InfSmall
M−4 = −InfSmall    M−5 = +Any    M−6 = −Any
```

This is maximal saturation — *every* bit stores value. The price is
asymmetry, and the original sketch's arithmetic needs correcting: for `3F`
(`F = 8`, 16 payload bits) the largest normal becomes
`(2^16 − 8)/2^8 = 255.96875` (not 248/247), i.e. the format loses its top
**7 ulps**, and the smallest positive stays `2^−8 = 1/256` (not `1/555`).
Signs still have to live somewhere, giving two sub-variants:

* **C1, sign-magnitude**: one payload bit becomes the sign → the magnitude
  range halves to `F−1.F` fixed-point (asymmetric with the spec).
* **C2, two's-complement**: range becomes `[−2^(F−1), 2^(F−1))` — again
  half the spec'd magnitude, and `−0` disappears but so does the clean
  `FFFF ii…i.ff…f` bit picture.

Runtime costs: every classification is a **multi-limb compare** against
`2^2F − 8`; `pack` must round *around* the stolen codes (overflow now
triggers 7 ulps early); `is_nan` is a full-width compare. Everything the
current format does in O(1) on one flags word becomes O(words).

### The alignment punchline

The implementation stores numbers 64-bit aligned:
`words = ⌈2F/64⌉ + 1`. For every `N ≥ 6` the payload `2F` is already a
multiple of 64, so **any** nonzero number of flag bits — 1, 2, 3 or 4 —
costs exactly the same **one extra limb**. Schemes A′ and B save *zero
actual RAM* for all N ≥ 6; they only shave bits that padding restores.
Scheme C alone saves the flags limb (8 bytes/value, making storage a
perfect power of two), at the price of turning every O(1) flag probe into
an O(words) compare and complicating rounding at the top of the range.

### Verdict

The current `2F + 4` layout is deliberately chosen: it is **≤ 2 bits above
the information floor and ≤ 1 limb above the alignment floor**, all seven
specials are **orthogonal 1-bit probes** on a single flags word
(`NaN`/`Inf`/`Eps`/`Sign`, with `Inf∧Eps = Any` as the only compound), the
full spec'd range `F.F` survives symmetrically, and the printed bit layout
(`FFFF iiii.ffff`) matches the format definition 1:1. The alternatives
trade correctness-critical simplicity (and, for B/C, per-operation speed)
for at most 8 bytes per value.

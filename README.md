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
(`make test N=15` additionally runs it at precision 15), `make confirm32`
— in-memory random-add proof at N=32, `make clean` — remove artifacts.

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

| N  | layout                     | total bits |
|----|----------------------------|------------|
| 0  | `FFFF b.b`                 | 6          |
| 1  | `FFFF bb.bb`               | 8          |
| 2  | `FFFF bbbb.bbbb`           | 12         |
| 3  | `FFFF b^8.b^8`             | 20         |
| 4  | `FFFF b^16.b^16`           | 36         |
| 7  | `FFFF b^128.b^128`         | 260        |
| 10 | `FFFF b^1024.b^1024`       | 2052       |
| 20 | `FFFF b^1048576.b^1048576` | 2097156    |

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

`Any` round-trips through strings (`any`, `-any` parse; prints `+Any`) and
through bit strings (`0110-...-...`).

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
| **+∞** | ? | = | > | ? | > | > | > | > | > | > |
| **−∞** | ? | < | = | < | ? | < | < | < | < | < |
| **+A** | ? | ? | > | ? | > | ? | > | > | ? | > |
| **−A** | ? | < | ? | < | ? | < | ? | < | < | ? |
| **+ε** | ? | < | > | ? | > | = | > | > | < | > |
| **−ε** | ? | < | > | < | ? | < | = | < | < | > |
| **0**  | ? | < | > | < | > | < | > | = | < | > |
| **+x** | ? | < | > | ? | > | > | > | > | cmp | > |
| **−x** | ? | < | > | < | ? | < | < | < | < | cmp |

`±Any` is ordered only against values strictly on the other side of zero
(and against zero itself); against anything on its own side the answer is
`?`. `eq` is `cmp == Equal`, so `NaN` and `±Any` are never equal to
anything, including themselves.

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
  `nan`.
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

## Tests

```console
$ make test        # = cargo test (the fixed-N suite below)
$ make test N=15   # the same suite, additionally run at precision N=15
```

The suite (**94 tests**) is deliberately exhaustive:

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
* **library unit tests** (49) — big-int properties, packing/rounding,
  special algebra, transcendentals vs 80-digit references at N=7/8, complex
  identities (Euler, i^i, branch cuts), parser precedence/errors, across
  `N ∈ {1, 2, 3, 6, 7, 8}` with spot checks at N=0/10/12.
* **CLI integration tests** (13) — argument handling, exit codes, `--bits`,
  `--from-bits` (including malformed input → exit 2), `--quiet`, `±Any`
  output, N=18 end-to-end, and the spec's own examples.

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
| huge N end-to-end | `nc 18 "2^100"` | `1267650600228229401496703205376` |
| quiet compute (no printing) | `nc 32 -q "…"` | *(nothing; exit 0)* |
| 32C in memory (8.6G-bit numbers) | `make confirm32` | `OK: 32C addition confirmed in memory` (8 GiB peak, 8.81 s) |

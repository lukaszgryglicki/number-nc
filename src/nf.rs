//! The NF number layer.
//!
//! # Global precision `N`
//!
//! `N` is identical for every number in one invocation, so it is a single
//! process-wide configuration value ([`set_n`] / [`get_n`]) and is **not**
//! stored inside numbers.  With `F = 2^N`:
//!
//! * the part before the decimal point has `F` bits,
//! * the part after the decimal point has `F` bits,
//! * 4 special bits always exist: NaN, Inf (overflow), Eps (underflow /
//!   "InfSmall"), Sign (`0` = positive, `1` = negative).
//!
//! Logical size: `2F + 4` bits.  `N` is unbounded - the only limit is RAM.
//!
//! # Representation: raw machine words
//!
//! A number is a bare `Vec<u64>` of [`words`]`()` machine words:
//!
//! ```text
//! index 0 .. L-1 : mantissa limbs, little-endian, L = ceil(2F/64)
//!                  value = mantissa / 2^F   (fixed point, sign-magnitude)
//! index L (last) : FLAGS word: bit0 NaN, bit1 Inf, bit2 Eps, bit3 Sign
//! ```
//!
//! The mantissa is 64-bit aligned for speed; [`to_bits`] / [`from_bits`]
//! convert to/from the exact packed `2F+4`-bit logical form.
//!
//! # Special-value semantics (the "limit interpretation")
//!
//! `Inf` means "the true value overflowed": it behaves like a signed
//! infinity.  `Eps` ("InfSmall") means "the true value is non-zero but
//! smaller in magnitude than the smallest representable value `2^-F`": it
//! behaves like a signed infinitesimal.  All operations treat them as the
//! corresponding limits; combinations whose limit is indeterminate give NaN:
//!
//! ```text
//! Inf + Inf(same sign)=Inf   Inf + -Inf = NaN      Inf + finite = Inf
//! Eps + Eps(same sign)=Eps   Eps + -Eps = NaN      Eps + x = x   Eps + 0 = Eps
//! Inf * Inf = Inf            Inf * x = Inf         Inf * 0 = NaN  Inf * Eps = Any
//! Eps * Eps = Eps            Eps * x = Eps         Eps * 0 = 0
//! anything / 0   = NaN  (zero carries no sign, so the sign of the
//!                        resulting infinity would be unknowable)
//! 0 / nonzero    = 0         x / Inf = Eps         Inf / Inf = Any
//! x / Eps = Inf              Eps / Eps = Any       Eps / Inf = Eps,  Inf / Eps = Inf
//! ```
//!
//! `Any` (Inf and Eps bits both set) means "sign known, magnitude anywhere
//! in `(0, Inf]`" — used when a form is indeterminate but its sign is not.
//!
//! A normal result that overflows becomes `Inf`; a non-zero result that
//! rounds to zero becomes `Eps`.  Rounding is round-to-nearest, ties away
//! from zero.

use crate::fx::Fx;
use crate::limb;
use crate::limb::Limb;
use std::cmp::Ordering;
use std::sync::atomic::{AtomicU32, Ordering as AtOrd};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// global N configuration
// ---------------------------------------------------------------------------

static N_VAL: AtomicU32 = AtomicU32::new(0);
static N_LOCK: Mutex<()> = Mutex::new(());

/// Set the global precision parameter `N` for this invocation.
/// Call once before doing any arithmetic.
pub fn set_n(n: u32) {
    N_VAL.store(n, AtOrd::SeqCst);
}

/// The current global `N`.
#[inline]
pub fn get_n() -> u32 {
    N_VAL.load(AtOrd::SeqCst)
}

/// Run `f` with the global `N` temporarily set to `n` (serialized by a global
/// lock; mainly useful for tests and multi-precision experiments).
pub fn with_n<T>(n: u32, f: impl FnOnce() -> T) -> T {
    let _g = N_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let old = get_n();
    set_n(n);
    let r = f();
    set_n(old);
    r
}

/// `F = 2^N`: number of bits before the point == number after the point.
///
/// There is no artificial upper bound on `N`; if `2^N` cannot even be
/// addressed on this machine the process aborts (equivalent to OOM, which is
/// the accepted limit).
#[inline]
pub fn frac_bits() -> usize {
    1usize
        .checked_shl(get_n())
        .expect("NF: 2^N exceeds the address space of this machine")
}

/// Total mantissa bits `2F` (integer part + fractional part).
#[inline]
pub fn total_bits() -> usize {
    frac_bits()
        .checked_mul(2)
        .expect("NF: 2^(N+1) exceeds the address space of this machine")
}

/// Number of 64-bit mantissa limbs `L = ceil(2F/64)`.
#[inline]
pub fn mant_limbs() -> usize {
    total_bits().div_ceil(limb::LIMB_BITS).max(1)
}

/// Total machine words per number: mantissa limbs + 1 flags word.
#[inline]
pub fn words() -> usize {
    mant_limbs() + 1
}

/// Logical size in bits: `2*(2^N) + 4`.
#[inline]
pub fn logical_bits() -> usize {
    total_bits() + 4
}

// ---------------------------------------------------------------------------
// flags word
// ---------------------------------------------------------------------------

/// Flags-word bit: NaN (number is invalid).
pub const FLAG_NAN: u64 = 1 << 0;
/// Flags-word bit: Inf (overflow of the integer part; +/-Inf with sign).
pub const FLAG_INF: u64 = 1 << 1;
/// Flags-word bit: Eps / InfSmall (non-zero underflow; +/-InfSmall with sign).
pub const FLAG_EPS: u64 = 1 << 2;
/// Flags-word bit: sign, `1` = negative (`0` = positive).
pub const FLAG_NEG: u64 = 1 << 3;

#[inline]
fn flags_of(a: &[Limb]) -> u64 {
    a[a.len() - 1]
}

/// Mantissa limbs of a number (without the flags word).
#[inline]
pub fn mant(a: &[Limb]) -> &[Limb] {
    &a[..a.len() - 1]
}

/// The number is NaN.
#[inline]
pub fn is_nan(a: &[Limb]) -> bool {
    flags_of(a) & FLAG_NAN != 0
}

/// The number is +/-Inf **only** (overflow marker; Eps bit clear).
#[inline]
pub fn is_inf(a: &[Limb]) -> bool {
    flags_of(a) & (FLAG_NAN | FLAG_INF | FLAG_EPS) == FLAG_INF
}

/// The number is +/-InfSmall (aka eps) **only** (Inf bit clear).
#[inline]
pub fn is_eps(a: &[Limb]) -> bool {
    flags_of(a) & (FLAG_NAN | FLAG_INF | FLAG_EPS) == FLAG_EPS
}

/// The number is +/-Any: **both** the Inf and InfSmall bits are set.
/// `+Any` means "some value in `(0, +Inf]`" - the sign is known but the
/// magnitude is completely indeterminate (it may be InfSmall-tiny, any
/// normal value, or Inf-huge).  Produced by indeterminate forms whose sign
/// is nevertheless determined, e.g. `+Inf * +InfSmall`, `+Inf / +Inf`,
/// `+InfSmall / +InfSmall`.
#[inline]
pub fn is_any(a: &[Limb]) -> bool {
    flags_of(a) & (FLAG_NAN | FLAG_INF | FLAG_EPS) == (FLAG_INF | FLAG_EPS)
}

/// Sign bit is set (negative).  Meaningful for normal, Inf and Eps values.
#[inline]
pub fn is_neg(a: &[Limb]) -> bool {
    flags_of(a) & FLAG_NEG != 0
}

/// No special flag: a normal (finite, representable) number, possibly zero.
#[inline]
pub fn is_normal(a: &[Limb]) -> bool {
    flags_of(a) & (FLAG_NAN | FLAG_INF | FLAG_EPS) == 0
}

/// Normal and exactly zero.
#[inline]
pub fn is_zero(a: &[Limb]) -> bool {
    is_normal(a) && limb::is_zero(mant(a))
}

/// Normal and non-zero.
#[inline]
pub fn is_normal_nonzero(a: &[Limb]) -> bool {
    is_normal(a) && !limb::is_zero(mant(a))
}

// ---------------------------------------------------------------------------
// constructors
// ---------------------------------------------------------------------------

fn raw(flags: u64) -> Vec<Limb> {
    let mut v = vec![0; words()];
    let l = v.len();
    v[l - 1] = flags;
    v
}

/// `+0`.
pub fn zero() -> Vec<Limb> {
    raw(0)
}

/// NaN.
pub fn nan() -> Vec<Limb> {
    raw(FLAG_NAN)
}

/// +/-Inf.
pub fn inf(negative: bool) -> Vec<Limb> {
    raw(FLAG_INF | if negative { FLAG_NEG } else { 0 })
}

/// +/-InfSmall.
/// `+InfSmall` / `-InfSmall` (the underflow marker, aka `eps`).
pub fn eps(negative: bool) -> Vec<Limb> {
    raw(FLAG_EPS | if negative { FLAG_NEG } else { 0 })
}

/// `+Any` / `-Any`: sign known, magnitude indeterminate in `(0, +Inf]`.
/// Encoded with the Inf **and** InfSmall bits both set.
pub fn any(negative: bool) -> Vec<Limb> {
    raw(FLAG_INF | FLAG_EPS | if negative { FLAG_NEG } else { 0 })
}

/// Smallest representable positive value, `2^-F` (one ulp).
pub fn min_pos() -> Vec<Limb> {
    let mut v = zero();
    v[0] = 1;
    v
}

/// Largest representable value: all `2F` mantissa bits set.
pub fn max_value() -> Vec<Limb> {
    let mut v = zero();
    let tb = total_bits();
    let l = mant_limbs();
    for w in v[..l].iter_mut() {
        *w = u64::MAX;
    }
    let rem = tb % limb::LIMB_BITS;
    if rem != 0 {
        v[l - 1] = (1u64 << rem) - 1;
    }
    v
}

/// Pack sign + raw mantissa magnitude (scale `2^-F`) into a number.
/// Overflow (more than `2F` bits) gives Inf; a zero magnitude gives `Eps`
/// when `nonzero_hint` says the true value was non-zero, else `+0`.
pub fn pack(negative: bool, m: &[Limb], nonzero_hint: bool) -> Vec<Limb> {
    if limb::bit_len(m) > total_bits() {
        return inf(negative);
    }
    if limb::is_zero(m) {
        return if nonzero_hint { eps(negative) } else { zero() };
    }
    let mut v = vec![0; words()];
    let l = mant_limbs();
    let ml = limb::eff_len(m);
    v[..ml].copy_from_slice(&m[..ml]);
    if negative {
        v[l] = FLAG_NEG;
    }
    v
}

/// Exact integer constructor; values above the largest representable become Inf.
pub fn from_u64(v: u64) -> Vec<Limb> {
    pack(false, &limb::shl(&limb::from_u64(v), frac_bits()), false)
}

/// Exact integer constructor.
pub fn from_i64(v: i64) -> Vec<Limb> {
    pack(
        v < 0,
        &limb::shl(&limb::from_u64(v.unsigned_abs()), frac_bits()),
        false,
    )
}

/// `1`.
pub fn one() -> Vec<Limb> {
    from_u64(1)
}

// ---------------------------------------------------------------------------
// sign operations
// ---------------------------------------------------------------------------

/// `-a`.
pub fn neg(a: &[Limb]) -> Vec<Limb> {
    let mut v = a.to_vec();
    let l = v.len() - 1;
    if is_nan(a) || is_zero(a) {
        return v;
    }
    v[l] ^= FLAG_NEG;
    v
}

/// `|a|`.
pub fn abs(a: &[Limb]) -> Vec<Limb> {
    let mut v = a.to_vec();
    let l = v.len() - 1;
    v[l] &= !FLAG_NEG;
    v
}

/// Copy with the given sign (specials keep their kind).
pub fn with_sign(a: &[Limb], negative: bool) -> Vec<Limb> {
    let mut v = a.to_vec();
    let l = v.len() - 1;
    if is_nan(a) || is_zero(a) {
        return v;
    }
    if negative {
        v[l] |= FLAG_NEG;
    } else {
        v[l] &= !FLAG_NEG;
    }
    v
}

// ---------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------

/// `a + b`.
pub fn add(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    // Any operands: sign-known indeterminate magnitude in (0, Inf].
    //   +Any + {0, +x, +Eps, +Any} = +Any     (result stays in (0, Inf])
    //   +Any + +Inf                = +Inf     ((M,Inf] + (0,Inf] = (M,Inf])
    //   +Any + {negative anything} = NaN      (sign of the sum unknowable)
    if is_any(a) || is_any(b) {
        let (x, y) = if is_any(a) { (a, b) } else { (b, a) };
        if is_zero(y) {
            return x.to_vec();
        }
        if is_neg(x) != is_neg(y) {
            return nan();
        }
        return if is_inf(y) { y.to_vec() } else { x.to_vec() };
    }
    if is_inf(a) {
        if is_inf(b) && is_neg(a) != is_neg(b) {
            return nan(); // Inf - Inf
        }
        return a.to_vec();
    }
    if is_inf(b) {
        return b.to_vec();
    }
    if is_eps(a) {
        if is_eps(b) {
            return if is_neg(a) == is_neg(b) { a.to_vec() } else { nan() };
        }
        return if is_zero(b) { a.to_vec() } else { b.to_vec() };
    }
    if is_eps(b) {
        return if is_zero(a) { b.to_vec() } else { a.to_vec() };
    }
    // both normal
    let (ma, mb) = (mant(a), mant(b));
    let (na, nb) = (is_neg(a), is_neg(b));
    if na == nb {
        pack(na, &limb::add(ma, mb), false)
    } else {
        match limb::cmp(ma, mb) {
            Ordering::Equal => zero(),
            Ordering::Greater => pack(na, &limb::sub(ma, mb), false),
            Ordering::Less => pack(nb, &limb::sub(mb, ma), false),
        }
    }
}

/// `a - b`.
pub fn sub(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    add(a, &neg(b))
}

/// `a * b`.
pub fn mul(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    let sign = is_neg(a) != is_neg(b);
    // exact zeros: 0 * {Inf, Any} = NaN (the other side may be truly
    // infinite), 0 * anything-else = 0
    if is_zero(a) || is_zero(b) {
        return if is_inf(a) || is_inf(b) || is_any(a) || is_any(b) {
            nan()
        } else {
            zero()
        };
    }
    // Any * {nonzero anything} = Any: (0,Inf] * positive-set = (0,Inf]
    if is_any(a) || is_any(b) {
        return any(sign);
    }
    if is_inf(a) || is_inf(b) {
        // Inf * Eps = Any (indeterminate magnitude, determinate sign);
        // Inf * (Inf|normal nonzero) = Inf
        if is_eps(a) || is_eps(b) {
            return any(sign);
        }
        return inf(sign);
    }
    if is_eps(a) || is_eps(b) {
        // Eps * Eps = Eps, Eps * normal nonzero = Eps
        return eps(sign);
    }
    let prod = limb::mul(mant(a), mant(b)); // scale 2^-2F
    let m = limb::round_shr(&prod, frac_bits());
    pack(sign, &m, true)
}

/// `a / b`.
pub fn div(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    let sign = is_neg(a) != is_neg(b);
    if is_zero(b) {
        return nan(); // sign of the infinity would be unknowable (no signed zero)
    }
    if is_zero(a) {
        return zero(); // note: 0 / Any = 0 is sound - Any is never zero
    }
    // Any anywhere: quotient magnitude indeterminate, sign known
    if is_any(a) || is_any(b) {
        return any(sign);
    }
    if is_inf(b) {
        return if is_inf(a) { any(sign) } else { eps(sign) };
    }
    if is_eps(b) {
        return if is_eps(a) { any(sign) } else { inf(sign) };
    }
    if is_inf(a) {
        return inf(sign);
    }
    if is_eps(a) {
        return eps(sign);
    }
    // both normal non-zero: round((a << F) / b) with one extra bit for the tie
    let num = limb::shl(mant(a), frac_bits() + 1);
    let q = limb::div_rem(&num, mant(b)).0;
    let m = limb::round_shr(&q, 1);
    pack(sign, &m, true)
}

// ---------------------------------------------------------------------------
// comparison
// ---------------------------------------------------------------------------

/// Ordering rank; `-Inf < negatives < -Eps < 0 < +Eps < positives < +Inf`.
fn rank(a: &[Limb]) -> i32 {
    if is_inf(a) {
        return if is_neg(a) { 0 } else { 6 };
    }
    if is_eps(a) {
        return if is_neg(a) { 2 } else { 4 };
    }
    if limb::is_zero(mant(a)) {
        return 3;
    }
    if is_neg(a) {
        1
    } else {
        5
    }
}

/// Partial comparison: `None` if either operand is NaN, `None` when an
/// `Any` operand makes the order unknowable (an `Any` compares decisively
/// only against zero and against the opposite sign), and `None` for two
/// `Inf` (or two `Eps`) of the same sign: each stands for a *different*
/// unknown magnitude past the same edge of the range, so no order and no
/// equality between them can be claimed -- consistent with
/// `Inf - Inf = NaN` and `Inf / Inf = Any`.  Only zero and normal values
/// can ever be `Equal`.
pub fn cmp(a: &[Limb], b: &[Limb]) -> Option<Ordering> {
    if is_nan(a) || is_nan(b) {
        return None;
    }
    if is_any(a) || is_any(b) {
        // sign side of zero: -1 for <0, 0 for =0, +1 for >0
        let side = |v: &[Limb]| -> i32 {
            if is_zero(v) {
                0
            } else if is_neg(v) {
                -1
            } else {
                1
            }
        };
        let (sa, sb) = (side(a), side(b));
        return if sa != sb { Some(sa.cmp(&sb)) } else { None };
    }
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return Some(ra.cmp(&rb));
    }
    match ra {
        1 => Some(limb::cmp(mant(b), mant(a))),
        5 => Some(limb::cmp(mant(a), mant(b))),
        3 => Some(Ordering::Equal), // exact zero == exact zero
        _ => None, // same-sign Inf/Inf or Eps/Eps: two different unknowns
    }
}

/// Equality; only zero and normal values can be equal (`NaN`, `±Inf`,
/// `±Eps` and `±Any` are never equal to anything, including themselves).
pub fn eq(a: &[Limb], b: &[Limb]) -> bool {
    cmp(a, b) == Some(Ordering::Equal)
}

// ---------------------------------------------------------------------------
// integral operations
// ---------------------------------------------------------------------------

/// Normal and with an all-zero fractional part.
pub fn is_integral(a: &[Limb]) -> bool {
    if !is_normal(a) {
        return false;
    }
    let f = frac_bits();
    let m = mant(a);
    (0..f.div_ceil(limb::LIMB_BITS)).all(|i| {
        let bits = if (i + 1) * limb::LIMB_BITS <= f {
            u64::MAX
        } else {
            (1u64 << (f % limb::LIMB_BITS)) - 1
        };
        i >= m.len() || m[i] & bits == 0
    })
}

fn clear_frac(m: &[Limb]) -> Vec<Limb> {
    let f = frac_bits();
    limb::shl(&limb::shr(m, f), f)
}

/// Largest integer `<= a`.
pub fn floor(a: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_inf(a) {
        return a.to_vec();
    }
    if is_any(a) {
        // floor(+Any) could be 0 or any positive integer -> unknowable sign
        // marker fails -> NaN; floor(-Any) stays a negative integer -> -Any
        return if is_neg(a) { a.to_vec() } else { nan() };
    }
    if is_eps(a) {
        // (0, minpos) -> 0 ; (-minpos, 0) -> -1
        return if is_neg(a) { from_i64(-1) } else { zero() };
    }
    let m = mant(a);
    let t = clear_frac(m);
    if !is_neg(a) {
        return pack(false, &t, false);
    }
    if limb::cmp(&t, &limb::trimmed(m)) == Ordering::Equal {
        return a.to_vec();
    }
    pack(true, &limb::add(&t, &limb::shl(&[1], frac_bits())), false)
}

/// Smallest integer `>= a`.
pub fn ceil(a: &[Limb]) -> Vec<Limb> {
    neg(&floor(&neg(a)))
}

/// Round toward zero to an integer.
pub fn trunc(a: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_inf(a) {
        return a.to_vec();
    }
    if is_any(a) {
        return nan(); // could round to 0 (unsigned) or stay nonzero
    }
    if is_eps(a) {
        return zero();
    }
    pack(is_neg(a), &clear_frac(mant(a)), false)
}

/// Round to the nearest integer, ties away from zero.
pub fn round(a: &[Limb]) -> Vec<Limb> {
    if is_nan(a) || is_inf(a) {
        return a.to_vec();
    }
    if is_any(a) {
        return nan(); // could round to 0 (unsigned) or stay nonzero
    }
    if is_eps(a) {
        return zero();
    }
    let f = frac_bits();
    let m = limb::add(mant(a), &limb::shl(&[1], f - 1));
    pack(is_neg(a), &clear_frac(&m), false)
}

// ---------------------------------------------------------------------------
// bridges to the fx working layer (used by the math engine)
// ---------------------------------------------------------------------------

/// Convert to a working value at scale `frac_bits() + guard`.
pub(crate) fn to_fx(a: &[Limb], guard: usize) -> Fx {
    Fx::new(is_neg(a), limb::shl(mant(a), guard))
}

/// Round a working value at scale `s` into an NF number.
/// `nonzero_hint` marks results known to be non-zero mathematically.
pub(crate) fn from_fx(v: &Fx, s: usize, nonzero_hint: bool) -> Vec<Limb> {
    let (negv, m) = v.round_to(s, frac_bits());
    pack(negv, &m, nonzero_hint || !v.is_zero())
}

// ---------------------------------------------------------------------------
// exact logical bit layout (2F + 4 bits)
// ---------------------------------------------------------------------------

/// Pack into the exact logical `2*(2^N)+4`-bit form, MSB-first per byte:
/// `[NaN, Inf, Eps, Sign, integer-part bits (MSB..LSB), fraction bits
/// (MSB..LSB)]`, zero-padded to whole bytes.
pub fn to_bits(a: &[Limb]) -> Vec<u8> {
    let tb = total_bits();
    let nbits = tb + 4;
    let mut out = vec![0u8; nbits.div_ceil(8)];
    let mut put = |i: usize, v: bool| {
        if v {
            out[i / 8] |= 1 << (7 - i % 8);
        }
    };
    let fl = flags_of(a);
    put(0, fl & FLAG_NAN != 0);
    put(1, fl & FLAG_INF != 0);
    put(2, fl & FLAG_EPS != 0);
    put(3, fl & FLAG_NEG != 0);
    let m = mant(a);
    for j in 0..tb {
        // logical stream order: integer MSB ... fraction LSB == mantissa bit (tb-1-j)
        put(4 + j, limb::get_bit(m, tb - 1 - j));
    }
    out
}

/// Inverse of [`to_bits`]; canonicalizes (NaN clears everything else,
/// Inf/Eps/Any clear the mantissa, an all-zero mantissa without flags is
/// `+0`; Inf and Eps bits together decode as `Any`).
pub fn from_bits(bytes: &[u8]) -> Vec<Limb> {
    let tb = total_bits();
    let get = |i: usize| -> bool {
        i / 8 < bytes.len() && bytes[i / 8] & (1 << (7 - i % 8)) != 0
    };
    if get(0) {
        return nan();
    }
    let negative = get(3);
    match (get(1), get(2)) {
        (true, true) => return any(negative),
        (true, false) => return inf(negative),
        (false, true) => return eps(negative),
        (false, false) => {}
    }
    let mut m = vec![0u64; mant_limbs()];
    for j in 0..tb {
        if get(4 + j) {
            let bit = tb - 1 - j;
            m[bit / limb::LIMB_BITS] |= 1 << (bit % limb::LIMB_BITS);
        }
    }
    pack(negative, &m, false)
}

/// Render as a raw 0/1 bit string: `FFFF-IIII...-DDDD...` — the 4 special
/// bits (NaN, Inf, InfSmall, Sign), then the `2^N` integer-part bits
/// (MSB first), then the `2^N` fraction bits (MSB first).
/// Example at N=3: `0001-00000010-00000000` is `-2`.
pub fn to_bit_string(a: &[Limb]) -> String {
    let tb = total_bits();
    let f = frac_bits();
    let fl = flags_of(a);
    let mut out = String::with_capacity(tb + 6);
    for flag in [FLAG_NAN, FLAG_INF, FLAG_EPS, FLAG_NEG] {
        out.push(if fl & flag != 0 { '1' } else { '0' });
    }
    out.push('-');
    let m = mant(a);
    for j in (0..tb).rev() {
        out.push(if limb::get_bit(m, j) { '1' } else { '0' });
        if j == f && j != 0 {
            out.push('-');
        }
    }
    out
}

/// Parse a bit string as produced by [`to_bit_string`] (separators `-`, `_`,
/// `.` and spaces are ignored).  Expects exactly `2*(2^N)+4` value bits.
pub fn from_bit_string(s: &str) -> Result<Vec<Limb>, String> {
    let bits: Vec<bool> = s
        .chars()
        .filter_map(|c| match c {
            '0' => Some(Ok(false)),
            '1' => Some(Ok(true)),
            '-' | '_' | '.' | ' ' => None,
            other => Some(Err(format!("invalid character '{}' in bit string", other))),
        })
        .collect::<Result<_, _>>()?;
    let tb = total_bits();
    if bits.len() != tb + 4 {
        return Err(format!(
            "bit string has {} bits, expected {} (= 2*(2^{})+4)",
            bits.len(),
            tb + 4,
            get_n()
        ));
    }
    if bits[0] {
        return Ok(nan());
    }
    let negative = bits[3];
    match (bits[1], bits[2]) {
        (true, true) => return Ok(any(negative)),
        (true, false) => return Ok(inf(negative)),
        (false, true) => return Ok(eps(negative)),
        (false, false) => {}
    }
    let mut m = vec![0u64; mant_limbs()];
    for (j, &b) in bits[4..].iter().enumerate() {
        if b {
            let bit = tb - 1 - j;
            m[bit / limb::LIMB_BITS] |= 1 << (bit % limb::LIMB_BITS);
        }
    }
    Ok(pack(negative, &m, false))
}

// ---------------------------------------------------------------------------
// string I/O
// ---------------------------------------------------------------------------

/// Default number of decimal fraction digits carried by this precision:
/// `ceil(F * log10(2))`.
pub fn default_digits() -> usize {
    ((frac_bits() as f64) * std::f64::consts::LOG10_2).ceil() as usize
}

/// Parse a decimal string: `[+-] digits [. digits] [(e|E)[+-]digits]`, or the
/// special values `nan`, `[+-]inf`/`infinity`, `[+-]eps`/`infsmall`,
/// `[+-]any`, `[+-]zero` (case-insensitive).  `-0` and `-zero` canonicalize
/// to `+0` like every other zero.  Values beyond the representable range
/// become `Inf`/`Eps` exactly as the semantics dictate.
pub fn from_str(s: &str) -> Result<Vec<Limb>, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty number".into());
    }
    let (negative, body) = match t.as_bytes()[0] {
        b'+' => (false, &t[1..]),
        b'-' => (true, &t[1..]),
        _ => (false, t),
    };
    let lower = body.to_ascii_lowercase();
    match lower.as_str() {
        "nan" => return Ok(nan()),
        "inf" | "infinity" => return Ok(inf(negative)),
        "eps" | "infsmall" => return Ok(eps(negative)),
        "any" => return Ok(any(negative)),
        "zero" => return Ok(zero()),
        _ => {}
    }
    // split off exponent
    let (mantissa_str, exp10) = match lower.find(['e']) {
        Some(p) => {
            let e: i64 = lower[p + 1..]
                .parse()
                .map_err(|_| format!("bad exponent in '{}'", s))?;
            (&body[..p], e)
        }
        None => (body, 0),
    };
    let mut int_digits = String::new();
    let mut frac_digits = String::new();
    let mut seen_dot = false;
    for c in mantissa_str.chars() {
        match c {
            '0'..='9' => {
                if seen_dot {
                    frac_digits.push(c);
                } else {
                    int_digits.push(c);
                }
            }
            '.' if !seen_dot => seen_dot = true,
            '_' => {}
            _ => return Err(format!("invalid character '{}' in number '{}'", c, s)),
        }
    }
    if int_digits.is_empty() && frac_digits.is_empty() {
        return Err(format!("no digits in number '{}'", s));
    }
    // D = all digits as a big integer; value = D * 10^(exp10 - frac_len)
    let digits: String = format!("{}{}", int_digits, frac_digits);
    let digits = digits.trim_start_matches('0').to_string();
    if digits.is_empty() {
        return Ok(zero());
    }
    let de = exp10 - frac_digits.len() as i64;
    let f = frac_bits();
    // cheap range pre-checks so absurd exponents don't try to allocate the world
    let dec10 = digits.len() as i64 + de; // value in [10^(dec10-1), 10^dec10)
    let max10 = ((f as f64) * std::f64::consts::LOG10_2) as i64; // max ~= 2^F
    if dec10 - 1 > max10 + 1 {
        return Ok(inf(negative));
    }
    if dec10 < -(max10 + 2) {
        return Ok(eps(negative));
    }
    // big-int from digit chunks
    let mut d: Vec<Limb> = vec![];
    for chunk in digits.as_bytes().chunks(19) {
        let v: u64 = std::str::from_utf8(chunk).unwrap().parse().unwrap();
        d = limb::mul_small(&d, 10u64.pow(chunk.len() as u32));
        limb::add_small_in_place(&mut d, v);
    }
    if de >= 0 {
        // exact: mantissa = D * 10^de << F
        let m = limb::shl(&limb::mul(&d, &limb::pow10(de as usize)), f);
        Ok(pack(negative, &m, true))
    } else {
        // mantissa = round(D << F / 10^-de), ties away from zero
        let num = limb::shl(&d, f + 1);
        let q = limb::div_rem(&num, &limb::pow10((-de) as usize)).0;
        let m = limb::round_shr(&q, 1);
        Ok(pack(negative, &m, true))
    }
}

/// Format with `digits` decimal fraction digits (rounded, trailing zeros
/// trimmed).  `None` selects the **shortest roundtrip** representation: the
/// fewest digits that parse back to exactly the same NF value.
pub fn to_string_digits(a: &[Limb], digits: Option<usize>) -> String {
    if is_nan(a) {
        return "NaN".into();
    }
    if is_inf(a) {
        return if is_neg(a) { "-Inf" } else { "+Inf" }.into();
    }
    if is_eps(a) {
        return if is_neg(a) { "-InfSmall" } else { "+InfSmall" }.into();
    }
    if is_any(a) {
        return if is_neg(a) { "-Any" } else { "+Any" }.into();
    }
    match digits {
        Some(d) => fixed_digits_str(a, d),
        None => shortest_str(a),
    }
}

/// Format with the default (shortest roundtrip) representation.
pub fn to_string(a: &[Limb]) -> String {
    to_string_digits(a, None)
}

fn render_decimal(negative: bool, k: &[Limb], d: usize) -> String {
    // value = k / 10^d
    let (int_part, frac_part) = if d == 0 {
        (limb::trimmed(k), vec![])
    } else {
        limb::div_rem(k, &limb::pow10(d))
    };
    let int_str = dec_str(&int_part);
    let mut frac_str = if d == 0 {
        String::new()
    } else {
        dec_str_padded(&frac_part, d)
    };
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    let mut out = String::new();
    if negative && (!limb::is_zero(&int_part) || !frac_str.is_empty()) {
        out.push('-');
    }
    out.push_str(&int_str);
    if !frac_str.is_empty() {
        out.push('.');
        out.push_str(&frac_str);
    }
    out
}

fn fixed_digits_str(a: &[Limb], d: usize) -> String {
    let f = frac_bits();
    // k = round(mant * 10^d / 2^F), ties away from zero
    let k = limb::round_shr(&limb::mul(mant(a), &limb::pow10(d)), f);
    render_decimal(is_neg(a), &k, d)
}

/// Shortest decimal string that parses back to exactly this value.
fn shortest_str(a: &[Limb]) -> String {
    let f = frac_bits();
    let m = limb::trimmed(mant(a));
    if m.is_empty() {
        return "0".into();
    }
    // candidate with d fraction digits, if it roundtrips
    let candidate = |d: usize| -> Option<Vec<Limb>> {
        let k = limb::round_shr(&limb::mul(&m, &limb::pow10(d)), f);
        // parse-back: m2 = round(k * 2^F / 10^d)
        let num = limb::shl(&k, f + 1);
        let q = limb::div_rem(&num, &limb::pow10(d)).0;
        let m2 = limb::round_shr(&q, 1);
        if limb::cmp(&m2, &m) == Ordering::Equal {
            Some(k)
        } else {
            None
        }
    };
    // The exact expansion of m/2^F needs exactly F - trailing_zero_bits(m)
    // fraction digits and always roundtrips, so it caps the search.  For
    // sparse values (small literals at huge N) this keeps printing O(len).
    let tzb = {
        let mut i = 0;
        while m[i] == 0 {
            i += 1;
        }
        i * 64 + m[i].trailing_zeros() as usize
    };
    let exact_d = f.saturating_sub(tzb);
    // roundtrip success is monotone in d -> binary search the smallest d
    let (mut lo, mut hi) = (0usize, (default_digits() + 2).min(exact_d));
    debug_assert!(candidate(hi).is_some());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if candidate(mid).is_some() {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let k = candidate(lo).expect("internal: shortest-roundtrip search failed");
    render_decimal(is_neg(a), &k, lo)
}

fn dec_str(v: &[Limb]) -> String {
    if limb::is_zero(v) {
        return "0".into();
    }
    const P19: u64 = 10_000_000_000_000_000_000;
    let mut chunks: Vec<u64> = vec![];
    let mut cur = limb::trimmed(v);
    while !limb::is_zero(&cur) {
        let (q, r) = limb::div_rem_small(&cur, P19);
        chunks.push(r);
        cur = q;
    }
    let mut s = format!("{}", chunks.pop().unwrap());
    while let Some(c) = chunks.pop() {
        s.push_str(&format!("{:019}", c));
    }
    s
}

fn dec_str_padded(v: &[Limb], width: usize) -> String {
    let s = dec_str(v);
    if s == "0" && width == 0 {
        return String::new();
    }
    format!("{}{}", "0".repeat(width.saturating_sub(s.len())), s)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &[Limb]) -> String {
        to_string(x)
    }
    fn p(x: &str) -> Vec<Limb> {
        from_str(x).unwrap()
    }

    /// The standard test matrix requested for this library: N=7 (260 logical
    /// bits, guaranteed multi-word) plus the extreme low end N=2 (12 bits).
    pub const TEST_NS: [u32; 6] = [1, 2, 3, 6, 7, 8];

    #[test]
    fn cross_n_sweep() {
        for &n in TEST_NS.iter() {
            with_n(n, || {
                let a = p("1.25");
                let b = p("0.75");
                // addition/subtraction are exact -> exact roundtrip identities
                assert!(eq(&sub(&add(&a, &b), &b), &a), "n={}", n);
                assert!(is_zero(&add(&a, &neg(&a))), "n={}", n);
                assert!(eq(&abs(&neg(&a)), &a), "n={}", n);
                // string roundtrips (shortest form must parse back exactly)
                for v in [&a, &b, &min_pos(), &max_value(), &neg(&max_value())] {
                    assert!(eq(&p(&s(v)), v), "n={} v={}", n, s(v));
                }
                // overflow / underflow semantics at every size
                assert!(is_inf(&add(&max_value(), &max_value())), "n={}", n);
                assert!(is_eps(&mul(&min_pos(), &min_pos())), "n={}", n);
                assert!(is_eps(&div(&min_pos(), &p("3"))), "n={}", n);
                // ordering chain
                let chain = [
                    inf(true), neg(&max_value()), eps(true), zero(),
                    eps(false), min_pos(), max_value(), inf(false),
                ];
                for i in 0..chain.len() {
                    for j in 0..chain.len() {
                        // Inf/Eps self-pairs are unordered (each is a
                        // different unknown magnitude past the same edge)
                        let want = if i == j && [0, 2, 4, 7].contains(&i) {
                            None
                        } else {
                            Some(i.cmp(&j))
                        };
                        assert_eq!(cmp(&chain[i], &chain[j]), want, "n={} {}v{}", n, i, j);
                    }
                }
                // logical bit layout + bit string roundtrips
                for v in [&a, &neg(&b), &min_pos(), &max_value(), &inf(true), &eps(false)] {
                    let sb = to_bit_string(v);
                    assert_eq!(to_bit_string(&from_bits(&to_bits(v))), sb, "n={}", n);
                    assert_eq!(to_bit_string(&from_bit_string(&sb).unwrap()), sb, "n={}", n);
                }
                assert_eq!(logical_bits(), 2 * (1usize << n) + 4);
            });
        }
    }

    #[test]
    fn bit_string_display() {
        with_n(3, || {
            // the example from the specification: -2 at N=3
            assert_eq!(to_bit_string(&p("-2")), "0001-00000010-00000000");
            assert_eq!(to_bit_string(&p("2.5")), "0000-00000010-10000000");
            assert_eq!(to_bit_string(&nan()), "1000-00000000-00000000");
            assert_eq!(to_bit_string(&inf(false)), "0100-00000000-00000000");
            assert_eq!(to_bit_string(&eps(true)), "0011-00000000-00000000");
            assert!(eq(
                &from_bit_string("0001-00000010-00000000").unwrap(),
                &p("-2")
            ));
            assert!(from_bit_string("0001-0000").is_err());
            assert!(from_bit_string("0001-00000010-0000000x").is_err());
        });
        with_n(1, || {
            // N=1: 8 logical bits total: FFFF-II-DD
            assert_eq!(to_bit_string(&p("1.5")), "0000-01-10");
            // -0.5 = mant 2/4 = binary 0b10 at F=2
            assert_eq!(to_bit_string(&p("-0.5")), "0001-00-10");
            assert_eq!(to_bit_string(&p("0.25")), "0000-00-01");
        });
    }

    #[test]
    fn sizes_match_spec() {
        // 0F=6, 1F=8, 2F=12, 3F=20, 4F=36, 10F=2052 logical bits
        for &(n, bits) in &[(0u32, 6usize), (1, 8), (2, 12), (3, 20), (4, 36), (10, 2052)] {
            with_n(n, || {
                assert_eq!(logical_bits(), bits);
                assert_eq!(to_bits(&zero()).len(), bits.div_ceil(8));
            });
        }
        with_n(10, || {
            assert_eq!(frac_bits(), 1024);
            assert_eq!(mant_limbs(), 32);
            assert_eq!(words(), 33);
        });
    }

    #[test]
    fn parse_format_roundtrip() {
        with_n(8, || {
            for v in [
                "0", "1", "-1", "3.140625", "-2.75", "12345", "0.5", "-0.5",
                "255.999", "0.0078125",
            ] {
                let x = p(v);
                let y = p(&s(&x));
                assert!(eq(&x, &y), "{} -> {} not stable", v, s(&x));
            }
            assert_eq!(s(&p("3.140625")), "3.140625"); // exact binary fraction
            assert_eq!(s(&p("+12345")), "12345");
            assert_eq!(s(&p("-0")), "0");
            assert_eq!(s(&p("1e3")), "1000");
            assert_eq!(s(&p("2.5e-2")), "0.025");
            assert_eq!(s(&p(".5")), "0.5");
            assert_eq!(s(&p("5.")), "5");
        });
    }

    #[test]
    fn parse_specials_and_ranges() {
        with_n(3, || {
            // F = 8: min_pos = 1/256 ~ 0.00390625, max ~ 255.996
            assert!(is_eps(&p("0.001")));
            assert!(is_eps(&p("-0.001")) && is_neg(&p("-0.001")));
            // 0.002 rounds up to min_pos = 1/256; shortest roundtrip is "0.004"
            let mp = p("0.002");
            assert!(eq(&mp, &min_pos()));
            assert_eq!(s(&mp), "0.004");
            assert!(eq(&p(&s(&mp)), &mp));
            assert!(is_inf(&p("300")));
            assert!(is_inf(&p("1e99")) && !is_neg(&p("1e99")));
            assert!(is_eps(&p("1e-99")));
            assert!(is_nan(&p("nan")));
            assert!(is_inf(&p("-inf")) && is_neg(&p("-inf")));
            assert!(is_eps(&p("+infsmall")));
            // shortest form of max_value must roundtrip exactly
            assert!(eq(&p(&s(&max_value())), &max_value()));
            assert_eq!(to_string_digits(&max_value(), Some(8)), "255.99609375");
        });
        with_n(8, || {
            assert!(is_inf(&p("1e78"))); // max ~ 1.16e77
            assert!(!is_inf(&p("1e77")));
            assert!(is_eps(&p("1e-78")));
        });
    }

    #[test]
    fn add_sub_basics() {
        with_n(8, || {
            assert_eq!(s(&add(&p("1.5"), &p("2.25"))), "3.75");
            assert_eq!(s(&sub(&p("1.5"), &p("2.25"))), "-0.75");
            assert_eq!(s(&add(&p("-1.5"), &p("1.5"))), "0");
            let big = max_value();
            assert!(is_inf(&add(&big, &one())));
            assert!(is_inf(&add(&neg(&big), &neg(&one()))) && is_neg(&add(&neg(&big), &neg(&one()))));
        });
    }

    #[test]
    fn mul_div_basics() {
        with_n(8, || {
            assert_eq!(s(&mul(&p("1.5"), &p("-2.5"))), "-3.75");
            assert_eq!(s(&div(&p("7"), &p("2"))), "3.5");
            // 1/3 rounds to nearest multiple of 2^-256
            let third = div(&one(), &p("3"));
            let back = mul(&third, &p("3"));
            // |1 - 3*(1/3)| <= 3 * halfulp
            let diff = abs(&sub(&back, &one()));
            assert!(cmp(&diff, &p("1e-76")) == Some(Ordering::Less));
            // overflow / underflow
            assert!(is_inf(&mul(&max_value(), &p("2"))));
            assert!(is_eps(&mul(&min_pos(), &p("0.4"))));
            assert!(is_eps(&div(&min_pos(), &p("3"))));
        });
    }

    #[test]
    fn special_algebra() {
        with_n(4, || {
            let i = inf(false);
            let ni = inf(true);
            let e = eps(false);
            let ne = eps(true);
            let x = p("2.5");
            let z = zero();
            // add
            assert!(is_nan(&add(&i, &ni)));
            assert!(is_inf(&add(&i, &x)) && !is_neg(&add(&i, &x)));
            assert!(is_nan(&add(&e, &ne)));
            assert!(is_eps(&add(&e, &z)) && !is_neg(&add(&e, &z)));
            assert!(eq(&add(&e, &x), &x));
            // mul
            assert!(is_nan(&mul(&i, &z)));
            assert!(is_any(&mul(&i, &e)) && !is_neg(&mul(&i, &e)));
            assert!(is_any(&mul(&i, &ne)) && is_neg(&mul(&i, &ne)));
            assert!(is_inf(&mul(&i, &ni)) && is_neg(&mul(&i, &ni)));
            assert!(is_eps(&mul(&e, &ne)) && is_neg(&mul(&e, &ne)));
            assert!(is_eps(&mul(&e, &x)));
            assert!(is_zero(&mul(&e, &z)));
            // div
            assert!(is_nan(&div(&x, &z)));
            assert!(is_nan(&div(&z, &z)));
            assert!(is_zero(&div(&z, &x)));
            assert!(is_any(&div(&i, &i)) && !is_neg(&div(&i, &i)));
            assert!(is_any(&div(&i, &ni)) && is_neg(&div(&i, &ni)));
            assert!(is_eps(&div(&x, &i)));
            assert!(is_inf(&div(&x, &e)));
            assert!(is_any(&div(&e, &ne)) && is_neg(&div(&e, &ne)));
            assert!(is_eps(&div(&e, &i)));
            assert!(is_inf(&div(&i, &e)));
            assert!(is_inf(&div(&i, &x)));
            assert!(is_eps(&div(&e, &x)));
            // NaN propagates
            assert!(is_nan(&add(&nan(), &x)));
            assert!(is_nan(&mul(&nan(), &i)));
        });
    }

    #[test]
    fn ordering() {
        with_n(6, || {
            let vals = [
                inf(true),
                p("-5"),
                eps(true),
                zero(),
                eps(false),
                p("0.25"),
                p("7"),
                inf(false),
            ];
            for i in 0..vals.len() {
                for j in 0..vals.len() {
                    // Inf/Eps self-pairs: unordered under strict semantics
                    let want = if i == j && [0, 2, 4, 7].contains(&i) {
                        None
                    } else {
                        Some(i.cmp(&j))
                    };
                    assert_eq!(cmp(&vals[i], &vals[j]), want, "{} vs {}", i, j);
                }
            }
            assert_eq!(cmp(&nan(), &zero()), None);
            assert!(!eq(&nan(), &nan()));
        });
    }

    #[test]
    fn floor_ceil_round_trunc() {
        with_n(6, || {
            assert_eq!(s(&floor(&p("2.7"))), "2");
            assert_eq!(s(&floor(&p("-2.1"))), "-3");
            assert_eq!(s(&ceil(&p("2.1"))), "3");
            assert_eq!(s(&ceil(&p("-2.7"))), "-2");
            assert_eq!(s(&trunc(&p("-2.7"))), "-2");
            assert_eq!(s(&round(&p("2.5"))), "3");
            assert_eq!(s(&round(&p("-2.5"))), "-3");
            assert_eq!(s(&round(&p("2.49"))), "2");
            assert_eq!(s(&floor(&eps(false))), "0");
            assert_eq!(s(&floor(&eps(true))), "-1");
            assert_eq!(s(&ceil(&eps(false))), "1");
            assert_eq!(s(&ceil(&eps(true))), "0");
            assert_eq!(s(&round(&eps(true))), "0");
            assert!(is_integral(&p("42")));
            assert!(!is_integral(&p("42.5")));
            assert!(!is_integral(&inf(false)));
        });
    }

    #[test]
    fn bits_roundtrip() {
        for n in [0u32, 1, 2, 3, 5, 8] {
            with_n(n, || {
                let cases = vec![
                    zero(),
                    one(),
                    neg(&one()),
                    min_pos(),
                    max_value(),
                    nan(),
                    inf(true),
                    eps(false),
                ];
                for c in cases {
                    let bits = to_bits(&c);
                    assert_eq!(bits.len(), (logical_bits()).div_ceil(8));
                    let back = from_bits(&bits);
                    assert_eq!(to_bit_string(&back), to_bit_string(&c), "n={}", n);
                }
            });
        }
    }

    #[test]
    fn tiny_n_still_works() {
        with_n(0, || {
            // F=1: values are k/2 with k in 0..=3 magnitude: 0, 0.5, 1, 1.5
            assert_eq!(s(&p("0.5")), "0.5");
            assert_eq!(s(&p("1.5")), "1.5");
            assert!(is_inf(&p("2")));
            assert!(is_eps(&p("0.2")));
            assert_eq!(s(&add(&p("0.5"), &p("1"))), "1.5");
            assert!(is_inf(&add(&p("1.5"), &p("0.5"))));
        });
    }
}

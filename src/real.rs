//! Real transcendental mathematics for NF numbers.
//!
//! Every function takes / returns raw NF word arrays (see [`crate::nf`]).
//! Internally values are lifted to the [`crate::fx`] working representation
//! at scale `s = F + GUARD` bits, computed with power series + argument
//! reduction, and rounded **once** at the end (ties away from zero), so
//! results are accurate to well under one ulp of the NF format.
//!
//! Special values follow the limit interpretation documented in [`crate::nf`]:
//! e.g. `exp(-Inf) = +InfSmall`, `ln(+InfSmall) = -Inf`, `sin(+/-Inf) = NaN`,
//! `atan(+/-Inf) = +/-pi/2`.

use crate::fx::Fx;
use crate::limb;
use crate::limb::Limb;
use crate::nf;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Guard bits used for all internal computations.
pub const GUARD: usize = 128;

/// Working scale for the current global N.
#[inline]
fn scale() -> usize {
    nf::frac_bits() + GUARD
}

#[inline]
fn fx_one(s: usize) -> Fx {
    Fx::from_u64(1, s)
}

// ---------------------------------------------------------------------------
// cached constants
// ---------------------------------------------------------------------------

/// Cache of computed constants keyed by (name, scale).
type ConstCache = Mutex<HashMap<(&'static str, usize), Vec<Limb>>>;

fn cache() -> &'static ConstCache {
    static CACHE: OnceLock<ConstCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(name: &'static str, s: usize, compute: impl FnOnce() -> Vec<Limb>) -> Fx {
    if let Some(m) = cache().lock().unwrap().get(&(name, s)) {
        return Fx::new(false, m.clone());
    }
    let v = compute();
    cache()
        .lock()
        .unwrap()
        .insert((name, s), v.clone());
    Fx::new(false, v)
}

/// `atan(1/k) * 2^s` by the Taylor series (terms shrink by `k^2` each step).
fn fx_atan_inv(k: u64, s: usize) -> Fx {
    let k2 = k * k;
    let mut t = Fx::from_u64(1, s).div_small(k);
    let mut sum = t.clone();
    let mut j = 1u64;
    let mut sign = true; // next term negative
    loop {
        t = t.div_small(k2);
        if t.is_zero() {
            break;
        }
        let term = t.div_small(2 * j + 1);
        if term.is_zero() {
            break;
        }
        sum = if sign { sum.sub(&term) } else { sum.add(&term) };
        sign = !sign;
        j += 1;
    }
    sum
}

/// `atanh(1/k) * 2^s` by the Taylor series.
fn fx_atanh_inv(k: u64, s: usize) -> Fx {
    let k2 = k * k;
    let mut t = Fx::from_u64(1, s).div_small(k);
    let mut sum = t.clone();
    let mut j = 1u64;
    loop {
        t = t.div_small(k2);
        if t.is_zero() {
            break;
        }
        let term = t.div_small(2 * j + 1);
        if term.is_zero() {
            break;
        }
        sum = sum.add(&term);
        j += 1;
    }
    sum
}

/// `pi * 2^s` - Machin: `pi = 16 atan(1/5) - 4 atan(1/239)`.
pub(crate) fn fx_pi(s: usize) -> Fx {
    cached("pi", s, || {
        let g = s + 32;
        let v = fx_atan_inv(5, g)
            .mul_small(16)
            .sub(&fx_atan_inv(239, g).mul_small(4));
        v.shr(32).mag
    })
}

/// `ln(2) * 2^s` - `ln2 = 18 atanh(1/26) - 2 atanh(1/4801) + 8 atanh(1/8749)`.
pub(crate) fn fx_ln2(s: usize) -> Fx {
    cached("ln2", s, || {
        let g = s + 32;
        let v = fx_atanh_inv(26, g)
            .mul_small(18)
            .sub(&fx_atanh_inv(4801, g).mul_small(2))
            .add(&fx_atanh_inv(8749, g).mul_small(8));
        v.shr(32).mag
    })
}

/// `e * 2^s`.
pub(crate) fn fx_e(s: usize) -> Fx {
    cached("e", s, || fx_exp(&fx_one(s + 32), s + 32).shr(32).mag)
}

/// `ln(10) * 2^s`.
pub(crate) fn fx_ln10(s: usize) -> Fx {
    cached("ln10", s, || {
        let g = s + 32;
        fx_ln(&Fx::from_u64(10, g), g).shr(32).mag
    })
}

// ---------------------------------------------------------------------------
// fx-level engines (inputs pre-checked by the NF-level wrappers)
// ---------------------------------------------------------------------------

const MAX_ITERS: usize = 1 << 22;

/// `exp(x)` at scale `s`.  Requires `|x| <= (F+4) * ln2`-ish so the result
/// shift stays small; NF wrappers enforce this.
pub(crate) fn fx_exp(x: &Fx, s: usize) -> Fx {
    if x.is_zero() {
        return fx_one(s);
    }
    let ln2 = fx_ln2(s);
    // k = round(x / ln2)  (fits easily in i64 after the range pre-check)
    let q = x.abs().div_s(&ln2, s); // |x|/ln2 at scale s
    let k_mag = limb::round_shr(&q.mag, s);
    debug_assert!(limb::bit_len(&k_mag) <= 63);
    let k_abs = *k_mag.first().unwrap_or(&0);
    let k_is_neg = x.neg;
    // r = x - k*ln2, |r| <= ln2/2 (+tiny slack)
    let kl = Fx::new(k_is_neg, ln2.mul_small(k_abs).mag);
    let r = x.sub(&kl);
    // reduce further: r' = r / 2^H, exp(r) = exp(r')^(2^H)
    const H: usize = 32;
    let rp = r.shr(H);
    let mut term = fx_one(s);
    let mut sum = fx_one(s);
    let mut j = 1u64;
    loop {
        term = term.mul_rs(&rp, s).div_small(j);
        if term.is_zero() {
            break;
        }
        sum = sum.add(&term);
        j += 1;
        assert!((j as usize) < MAX_ITERS, "exp series did not converge");
    }
    let mut v = sum;
    for _ in 0..H {
        v = v.mul_rs(&v, s);
    }
    // apply 2^k
    if k_is_neg {
        v.shr(k_abs as usize)
    } else {
        v.shl(k_abs as usize)
    }
}

/// `ln(x)` at scale `s` for `x > 0`.
pub(crate) fn fx_ln(x: &Fx, s: usize) -> Fx {
    assert!(!x.neg && !x.is_zero(), "fx_ln: requires x > 0");
    // normalize: x = mu * 2^e2 with mu in [1, 2)
    let bl = x.bit_len();
    let e2 = bl as i64 - s as i64 - 1;
    let mu = if e2 >= 0 {
        x.shr(e2 as usize)
    } else {
        x.shl((-e2) as usize)
    };
    // T sqrt reductions: y = mu^(1/2^T), then ln(mu) = 2^T * 2 * atanh((y-1)/(y+1))
    const T: usize = 16;
    let mut y = mu;
    for _ in 0..T {
        y = y.sqrt_s(s);
    }
    let one = fx_one(s);
    let z = y.sub(&one).div_s(&y.add(&one), s);
    let z2 = z.mul_rs(&z, s);
    let mut pow = z.clone();
    let mut sum = z.clone();
    let mut j = 1u64;
    loop {
        pow = pow.mul_rs(&z2, s);
        if pow.is_zero() {
            break;
        }
        let term = pow.div_small(2 * j + 1);
        if term.is_zero() {
            break;
        }
        sum = sum.add(&term);
        j += 1;
        assert!((j as usize) < MAX_ITERS, "ln series did not converge");
    }
    let ln_mu = sum.shl(T + 1);
    let e2fx = Fx::from_i64(e2, 0); // integer, scale 0
    ln_mu.add(&Fx::new(e2fx.neg, fx_ln2(s).mul_small(e2.unsigned_abs()).mag))
}

/// `sin(r)` at scale `s` for `|r| <= pi/4 + slack`.
fn fx_sin_series(r: &Fx, s: usize) -> Fx {
    let r2 = r.mul_rs(r, s);
    let mut term = r.clone();
    let mut sum = r.clone();
    let mut j = 1u64;
    let mut sign = true; // next term subtracted
    loop {
        term = term.mul_rs(&r2, s).div_small((2 * j) * (2 * j + 1));
        if term.is_zero() {
            break;
        }
        sum = if sign { sum.sub(&term) } else { sum.add(&term) };
        sign = !sign;
        j += 1;
        assert!((j as usize) < MAX_ITERS, "sin series did not converge");
    }
    sum
}

/// `cos(r)` at scale `s` for `|r| <= pi/4 + slack`.
fn fx_cos_series(r: &Fx, s: usize) -> Fx {
    let r2 = r.mul_rs(r, s);
    let mut term = fx_one(s);
    let mut sum = fx_one(s);
    let mut j = 1u64;
    let mut sign = true;
    loop {
        term = term.mul_rs(&r2, s).div_small((2 * j - 1) * (2 * j));
        if term.is_zero() {
            break;
        }
        sum = if sign { sum.sub(&term) } else { sum.add(&term) };
        sign = !sign;
        j += 1;
        assert!((j as usize) < MAX_ITERS, "cos series did not converge");
    }
    sum
}

/// Reduce `x` modulo `pi/2`: returns `(quadrant mod 4, remainder)` with
/// `|remainder| <= pi/4 + slack`, both at scale `s`.
fn fx_trig_reduce(x: &Fx, s: usize) -> (u32, Fx) {
    // pi with enough extra bits to survive the size of the integer part
    let extra = x.bit_len().saturating_sub(s) + 16;
    let s2 = s + extra;
    let halfpi = fx_pi(s2).shr(1);
    let xl = x.shl(extra).abs();
    // q = round(|x| / (pi/2)) as a plain big integer
    let (mut q, r) = limb::div_rem(&xl.mag, &halfpi.mag);
    let rem2 = limb::shl(&r, 1);
    if limb::cmp(&rem2, &halfpi.mag) != Ordering::Less {
        limb::add_small_in_place(&mut q, 1);
    }
    // r = |x| - q * pi/2 (signed), back at scale s
    let qpi = Fx::new(false, limb::mul(&q, &halfpi.mag));
    let rr = xl.sub(&qpi).shr(extra);
    let qm4 = (*q.first().unwrap_or(&0) & 3) as u32;
    if x.neg {
        // x = -(q * pi/2 + r): quadrant becomes (4 - q) mod 4, remainder -r
        ((4 - qm4) & 3, rr.neg())
    } else {
        (qm4, rr)
    }
}

/// `(sin(x), cos(x))` at scale `s`.
pub(crate) fn fx_sincos(x: &Fx, s: usize) -> (Fx, Fx) {
    let (quad, r) = fx_trig_reduce(x, s);
    let sn = fx_sin_series(&r, s);
    let cs = fx_cos_series(&r, s);
    match quad {
        0 => (sn, cs),
        1 => (cs, sn.neg()),
        2 => (sn.neg(), cs.neg()),
        _ => (cs.neg(), sn),
    }
}

/// `atan(x)` at scale `s` (any finite x).
pub(crate) fn fx_atan(x: &Fx, s: usize) -> Fx {
    if x.is_zero() {
        return Fx::zero();
    }
    let one = fx_one(s);
    let ax = x.abs();
    // |x| > 1: atan(x) = pi/2 - atan(1/x)
    if ax.cmp(&one) == Ordering::Greater {
        let inv = one.div_s(&ax, s);
        let inner = fx_atan(&inv, s);
        let v = fx_pi(s).shr(1).sub(&inner);
        return if x.neg { v.neg() } else { v };
    }
    // T halvings: atan(y) = 2 atan(y / (1 + sqrt(1 + y^2)))
    const T: usize = 8;
    let mut y = ax;
    for _ in 0..T {
        let y2 = y.mul_rs(&y, s);
        let sq = one.add(&y2).sqrt_s(s);
        y = y.div_s(&one.add(&sq), s);
    }
    let y2 = y.mul_rs(&y, s);
    let mut pow = y.clone();
    let mut sum = y.clone();
    let mut j = 1u64;
    let mut sign = true;
    loop {
        pow = pow.mul_rs(&y2, s);
        if pow.is_zero() {
            break;
        }
        let term = pow.div_small(2 * j + 1);
        if term.is_zero() {
            break;
        }
        sum = if sign { sum.sub(&term) } else { sum.add(&term) };
        sign = !sign;
        j += 1;
        assert!((j as usize) < MAX_ITERS, "atan series did not converge");
    }
    let v = sum.shl(T);
    if x.neg {
        v.neg()
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// NF-level wrappers
// ---------------------------------------------------------------------------

/// `pi` rounded to the current precision.
pub fn pi() -> Vec<Limb> {
    let s = scale();
    nf::from_fx(&fx_pi(s), s, true)
}

/// Euler's number `e` rounded to the current precision.
pub fn e_const() -> Vec<Limb> {
    let s = scale();
    nf::from_fx(&fx_e(s), s, true)
}

/// `ln(2)` rounded to the current precision.
pub fn ln2_const() -> Vec<Limb> {
    let s = scale();
    nf::from_fx(&fx_ln2(s), s, true)
}

/// `sqrt(a)`: NaN for negative values, `Eps -> Eps`, `Inf -> Inf`,
/// `+Any -> +Any`.  For normal values the result is correctly rounded
/// (exact integer sqrt).
pub fn sqrt(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_neg(a) && !nf::is_zero(a) {
        return nf::nan();
    }
    if nf::is_inf(a) || nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    // mantissa' = round(sqrt(m * 2^F)) - exact via integer sqrt
    let m = limb::isqrt_round(&limb::shl(nf::mant(a), nf::frac_bits()));
    nf::pack(false, &m, true)
}

/// `k`-th root.  `k=0` gives NaN; negative arguments are allowed for odd `k`.
pub fn nth_root(a: &[Limb], k: u64) -> Vec<Limb> {
    if k == 0 || nf::is_nan(a) {
        return nf::nan();
    }
    if k == 1 {
        return a.to_vec();
    }
    let negative = nf::is_neg(a);
    if negative && k.is_multiple_of(2) && !nf::is_zero(a) {
        return nf::nan();
    }
    if nf::is_inf(a) || nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec(); // sign preserved; only reachable for odd k when negative
    }
    if k == 2 {
        return sqrt(a);
    }
    let f = nf::frac_bits();
    let m = nf::mant(a);
    if k <= 64 {
        // exact: R = round((m * 2^(F(k-1)))^(1/k)) via the *2^k trick
        let shifted = limb::shl(m, f * (k as usize - 1) + k as usize);
        let r2 = limb::iroot(&shifted, k);
        let r = limb::round_shr(&r2, 1);
        nf::pack(negative, &r, true)
    } else {
        // huge k: root = exp(ln|a| / k), then restore the sign
        let s = scale();
        let v = fx_ln(&nf::to_fx(&nf::abs(a), GUARD), s).div_small(k);
        let r = fx_exp(&v, s);
        nf::from_fx(&if negative { r.neg() } else { r }, s, true)
    }
}

/// `exp(a)`.  `exp(+Inf)=+Inf`, `exp(-Inf)=+InfSmall`, `exp(+/-Eps)=1`,
/// `exp(+/-Any)=+Any` (`e^x > 0` always, magnitude unknown).
pub fn exp(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_any(a) {
        return nf::any(false);
    }
    if nf::is_inf(a) {
        return if nf::is_neg(a) { nf::eps(false) } else { nf::inf(false) };
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return nf::one();
    }
    let s = scale();
    let f = nf::frac_bits();
    let x = nf::to_fx(a, GUARD);
    // range check so the 2^k shift stays small: |x| >= (F+2)*ln2 saturates
    let lim = fx_ln2(s).mul_small(f as u64 + 2);
    if x.abs().cmp(&lim) != Ordering::Less {
        return if x.neg { nf::eps(false) } else { nf::inf(false) };
    }
    nf::from_fx(&fx_exp(&x, s), s, true)
}

/// Natural logarithm.  `ln(0) = ln(+Eps) = -Inf`, `ln(+Inf) = +Inf`,
/// negative arguments (including `-Eps`) give NaN, and `+/-Any` give NaN
/// (the sign of `ln` of an unknown magnitude is unknowable).
pub fn ln(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_neg(a) && !nf::is_zero(a) {
        return nf::nan();
    }
    if nf::is_zero(a) || nf::is_eps(a) {
        return nf::inf(true);
    }
    if nf::is_inf(a) {
        return nf::inf(false);
    }
    if nf::eq(a, &nf::one()) {
        return nf::zero();
    }
    let s = scale();
    nf::from_fx(&fx_ln(&nf::to_fx(a, GUARD), s), s, true)
}

/// Logarithm base 2.
pub fn log2(a: &[Limb]) -> Vec<Limb> {
    log_with_fx(a, fx_ln2)
}

/// Logarithm base 10.
pub fn log10(a: &[Limb]) -> Vec<Limb> {
    log_with_fx(a, fx_ln10)
}

fn log_with_fx(a: &[Limb], base_ln: impl Fn(usize) -> Fx) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_neg(a) && !nf::is_zero(a) {
        return nf::nan();
    }
    if nf::is_zero(a) || nf::is_eps(a) {
        return nf::inf(true);
    }
    if nf::is_inf(a) {
        return nf::inf(false);
    }
    if nf::eq(a, &nf::one()) {
        return nf::zero();
    }
    let s = scale();
    let v = fx_ln(&nf::to_fx(a, GUARD), s).div_s(&base_ln(s), s);
    nf::from_fx(&v, s, true)
}

/// `log_b(x) = ln(x) / ln(b)`.  Domain errors give NaN.
pub fn logb(b: &[Limb], x: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(b) || nf::is_nan(x) {
        return nf::nan();
    }
    // b must be positive normal and != 1 for a well-defined finite answer;
    // everything else goes through the div special semantics.
    let lb = ln(b);
    let lx = ln(x);
    nf::div(&lx, &lb)
}

/// Integer part of a normal integral NF number as (sign, u64) when it fits.
fn as_small_int(a: &[Limb]) -> Option<(bool, u64)> {
    if !nf::is_integral(a) {
        return None;
    }
    let int = limb::shr(nf::mant(a), nf::frac_bits());
    if limb::bit_len(&int) > 63 {
        return None;
    }
    Some((nf::is_neg(a), *int.first().unwrap_or(&0)))
}

/// Parity of an integral NF number (bit F of the mantissa = the 2^0 digit).
fn integral_is_odd(a: &[Limb]) -> bool {
    limb::get_bit(nf::mant(a), nf::frac_bits())
}

/// `x^k` at working precision by binary exponentiation, `k >= 1`.
/// Returns None on intermediate overflow (result magnitude certainly > max).
fn fx_powi(x: &Fx, mut k: u64, s: usize) -> Option<Fx> {
    let limit = s + nf::frac_bits() + 66; // magnitude bits before certain overflow
    let mut base = x.clone();
    let mut acc: Option<Fx> = None;
    loop {
        if k & 1 == 1 {
            acc = Some(match acc {
                None => base.clone(),
                Some(a) => a.mul_rs(&base, s),
            });
            if acc.as_ref().unwrap().bit_len() > limit {
                return None;
            }
        }
        k >>= 1;
        if k == 0 {
            break;
        }
        base = base.mul_rs(&base, s);
        if base.bit_len() > limit {
            // base overflow while bits of k remain -> result overflows
            // unless no remaining set bit needs it; handle conservatively:
            return None;
        }
    }
    Some(acc.unwrap())
}

/// `a^b` with the full special-value table; principal real semantics.
pub fn pow(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_nan(b) {
        return nf::nan();
    }
    // b == 0 (exact): x^0 = 1 for every non-NaN x (0^0 = 1 by convention)
    if nf::is_zero(b) {
        return nf::one();
    }
    let one = nf::one();
    // b = +/-Any: exponent magnitude unknown (could be Eps-tiny or Inf-huge)
    if nf::is_any(b) {
        if nf::is_zero(a) {
            // 0^(positive anything) = 0; 0^(negative anything) undefined
            return if nf::is_neg(b) { nf::nan() } else { nf::zero() };
        }
        if nf::is_neg(a) {
            return nf::nan(); // negative base, non-integral exponents possible
        }
        if nf::eq(a, &one) {
            return nf::nan(); // 1^(possibly Inf) is indeterminate, like 1^Inf
        }
        if nf::is_inf(a) && !nf::is_neg(b) {
            return nf::inf(false); // Inf^(any positive) = Inf
        }
        return nf::any(false); // positive base: result positive, size unknown
    }
    // a = +/-Any: base magnitude unknown
    if nf::is_any(a) {
        if nf::is_neg(a) {
            // negative unknown base: only exact integral exponents make sense
            if nf::is_normal(b) && nf::is_integral(b) {
                return nf::any(integral_is_odd(b));
            }
            return nf::nan();
        }
        return nf::any(false); // (0,Inf]^(nonzero b) stays in (0,Inf]
    }
    // x == 1 exact: 1^inf indeterminate, otherwise 1
    if nf::eq(a, &one) {
        return if nf::is_inf(b) { nf::nan() } else { one };
    }
    // b = +/-Eps: x^(->0) -> 1 for normal x > 0; 0^(+eps)=0, 0^(-eps)=NaN;
    // x<0 NaN; Inf^(+/-Eps) and Eps^(+/-Eps) are e^(+/-Any) = +Any
    if nf::is_eps(b) {
        if nf::is_zero(a) {
            return if nf::is_neg(b) { nf::nan() } else { nf::zero() };
        }
        if nf::is_neg(a) {
            return nf::nan();
        }
        if nf::is_inf(a) || nf::is_eps(a) {
            return nf::any(false);
        }
        return nf::one();
    }
    // b = +/-Inf
    if nf::is_inf(b) {
        let binf_neg = nf::is_neg(b);
        if nf::is_zero(a) {
            return if binf_neg { nf::nan() } else { nf::zero() };
        }
        if nf::is_eps(a) {
            if nf::is_neg(a) {
                return nf::nan(); // oscillating sign
            }
            return if binf_neg { nf::inf(false) } else { nf::eps(false) };
        }
        if nf::is_neg(a) {
            return nf::nan(); // (negative)^Inf oscillates
        }
        // |a| vs 1 decides
        let cmp1 = nf::cmp(a, &one).unwrap();
        return match (cmp1, binf_neg) {
            (Ordering::Greater, false) => nf::inf(false),
            (Ordering::Greater, true) => nf::eps(false),
            (Ordering::Less, false) => nf::eps(false),
            (Ordering::Less, true) => nf::inf(false),
            (Ordering::Equal, _) => unreachable!("a == 1 handled above"),
        };
    }
    // exact-zero base
    if nf::is_zero(a) {
        return if nf::is_neg(b) { nf::nan() } else { nf::zero() };
    }
    // integral exponent path (works for negative bases, Inf, Eps)
    if nf::is_integral(b) {
        let odd = integral_is_odd(b);
        let sign = nf::is_neg(a) && odd;
        let b_neg = nf::is_neg(b);
        if nf::is_inf(a) {
            return if b_neg { nf::eps(sign) } else { nf::inf(sign) };
        }
        if nf::is_eps(a) {
            return if b_neg { nf::inf(sign) } else { nf::eps(sign) };
        }
        if let Some((_, k)) = as_small_int(b) {
            let s = scale();
            let x = nf::to_fx(a, GUARD);
            return match fx_powi(&x, k, s) {
                None => {
                    if b_neg {
                        nf::eps(sign)
                    } else {
                        nf::inf(sign)
                    }
                }
                Some(v) => {
                    if b_neg {
                        if v.is_zero() {
                            // |a|^k underflowed the working scale => 1/that overflows
                            return nf::inf(sign);
                        }
                        nf::from_fx(&fx_one(s).div_s(&v, s), s, true)
                    } else {
                        nf::from_fx(&v, s, true)
                    }
                }
            };
        }
        // gigantic integral exponent: decide by |a| vs 1 (a != 1, a != 0)
        let cmp1 = nf::cmp(&nf::abs(a), &one).unwrap();
        let grows = cmp1 == Ordering::Greater;
        return match (grows, b_neg) {
            (true, false) => nf::inf(sign),
            (true, true) => nf::eps(sign),
            (false, false) => nf::eps(sign),
            (false, true) => nf::inf(sign),
        };
    }
    // non-integral exponent: base must be positive
    if nf::is_neg(a) {
        return nf::nan();
    }
    if nf::is_inf(a) {
        return if nf::is_neg(b) { nf::eps(false) } else { nf::inf(false) };
    }
    if nf::is_eps(a) {
        return if nf::is_neg(b) { nf::inf(false) } else { nf::eps(false) };
    }
    // a normal positive, b normal non-integral: exp(b * ln a)
    let s = scale();
    let f = nf::frac_bits();
    let la = fx_ln(&nf::to_fx(a, GUARD), s);
    let prod = la.mul_rs(&nf::to_fx(b, GUARD), s);
    let lim = fx_ln2(s).mul_small(f as u64 + 2);
    if prod.abs().cmp(&lim) != Ordering::Less {
        return if prod.neg { nf::eps(false) } else { nf::inf(false) };
    }
    nf::from_fx(&fx_exp(&prod, s), s, true)
}

/// `sin(a)`; `sin(+/-Inf) = sin(+/-Any) = NaN`, `sin(+/-Eps) = +/-Eps`.
pub fn sin(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let s = scale();
    let (sn, _) = fx_sincos(&nf::to_fx(a, GUARD), s);
    nf::from_fx(&sn, s, true)
}

/// `cos(a)`; `cos(+/-Inf) = cos(+/-Any) = NaN`, `cos(+/-Eps) = 1`.
pub fn cos(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return nf::one();
    }
    let s = scale();
    let (_, cs) = fx_sincos(&nf::to_fx(a, GUARD), s);
    nf::from_fx(&cs, s, true)
}

/// `tan(a) = sin(a)/cos(a)` computed at working precision.
pub fn tan(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let s = scale();
    let (sn, cs) = fx_sincos(&nf::to_fx(a, GUARD), s);
    if cs.is_zero() {
        // exactly the working-precision pole: true tan is astronomically
        // large with the sign of sin
        return nf::inf(sn.neg);
    }
    nf::from_fx(&sn.div_s(&cs, s), s, true)
}

/// `asin(a)`: domain `[-1, 1]`, `asin(+/-Eps) = +/-Eps`.
pub fn asin(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let one = nf::one();
    match nf::cmp(&nf::abs(a), &one).unwrap() {
        Ordering::Greater => nf::nan(),
        Ordering::Equal => {
            let s = scale();
            let h = fx_pi(s).shr(1);
            nf::from_fx(&if nf::is_neg(a) { h.neg() } else { h }, s, true)
        }
        Ordering::Less => {
            let s = scale();
            let x = nf::to_fx(a, GUARD);
            let one_fx = fx_one(s);
            let denom = one_fx.sub(&x.mul_rs(&x, s)).sqrt_s(s);
            let t = x.div_s(&denom, s);
            nf::from_fx(&fx_atan(&t, s), s, true)
        }
    }
}

/// `acos(a) = pi/2 - asin(a)` at working precision.
pub fn acos(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan();
    }
    let s = scale();
    let half_pi = fx_pi(s).shr(1);
    if nf::is_eps(a) || nf::is_zero(a) {
        return nf::from_fx(&half_pi, s, true);
    }
    let one = nf::one();
    match nf::cmp(&nf::abs(a), &one).unwrap() {
        Ordering::Greater => nf::nan(),
        Ordering::Equal => {
            if nf::is_neg(a) {
                nf::from_fx(&fx_pi(s), s, true)
            } else {
                nf::zero()
            }
        }
        Ordering::Less => {
            let x = nf::to_fx(a, GUARD);
            let one_fx = fx_one(s);
            let denom = one_fx.sub(&x.mul_rs(&x, s)).sqrt_s(s);
            let t = x.div_s(&denom, s);
            nf::from_fx(&half_pi.sub(&fx_atan(&t, s)), s, true)
        }
    }
}

/// `atan(a)`; `atan(+/-Inf) = +/-pi/2`, `atan(+/-Eps) = +/-Eps`,
/// `atan(+/-Any) = +/-Any` (the angle is in `(0, pi/2]`-ish, sign known).
pub fn atan(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let s = scale();
    if nf::is_inf(a) {
        let h = fx_pi(s).shr(1);
        return nf::from_fx(&if nf::is_neg(a) { h.neg() } else { h }, s, true);
    }
    nf::from_fx(&fx_atan(&nf::to_fx(a, GUARD), s), s, true)
}

/// `atan2(y, x)` - the angle of the point `(x, y)`; `atan2(0, 0) = NaN`.
pub fn atan2(y: &[Limb], x: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(y) || nf::is_nan(x) {
        return nf::nan();
    }
    // Any components: the angle is known only up to the half-plane of y.
    // y = 0 keeps the exact axis answers; otherwise the magnitude of the
    // angle is indeterminate but its sign matches the sign of y.
    if nf::is_any(y) || nf::is_any(x) {
        if nf::is_zero(y) {
            // x = +/-Any: on the positive side -> angle 0; negative -> pi
            return if nf::is_neg(x) {
                let s = scale();
                nf::from_fx(&fx_pi(s), s, true)
            } else {
                nf::zero()
            };
        }
        return nf::any(nf::is_neg(y));
    }
    let s = scale();
    let pi_fx = fx_pi(s);
    let y_zero = nf::is_zero(y);
    let x_zero = nf::is_zero(x);
    if y_zero && x_zero {
        return nf::nan();
    }
    if nf::is_inf(y) {
        if nf::is_inf(x) {
            return nf::nan();
        }
        let h = pi_fx.shr(1);
        return nf::from_fx(&if nf::is_neg(y) { h.neg() } else { h }, s, true);
    }
    if nf::is_inf(x) {
        // finite y
        if nf::is_neg(x) {
            let v = if nf::is_neg(y) && !y_zero { pi_fx.neg() } else { pi_fx };
            return nf::from_fx(&v, s, true);
        }
        return if y_zero { nf::zero() } else { nf::eps(nf::is_neg(y)) };
    }
    if x_zero {
        let h = pi_fx.shr(1);
        return nf::from_fx(&if nf::is_neg(y) { h.neg() } else { h }, s, true);
    }
    if nf::is_eps(x) && nf::is_eps(y) {
        return nf::nan();
    }
    if y_zero {
        return if nf::is_neg(x) {
            nf::from_fx(&pi_fx, s, true)
        } else {
            nf::zero()
        };
    }
    if nf::is_eps(y) {
        // x normal: angle ~ 0 or ~ pi
        return if nf::is_neg(x) {
            nf::from_fx(&if nf::is_neg(y) { pi_fx.neg() } else { pi_fx }, s, true)
        } else {
            nf::eps(nf::is_neg(y))
        };
    }
    if nf::is_eps(x) {
        // y normal: ~ +/- pi/2 regardless of the side of x
        let h = pi_fx.shr(1);
        return nf::from_fx(&if nf::is_neg(y) { h.neg() } else { h }, s, true);
    }
    // both normal non-zero
    let yv = nf::to_fx(y, GUARD);
    let xv = nf::to_fx(x, GUARD);
    let base = fx_atan(&yv.div_s(&xv, s), s);
    let v = if nf::is_neg(x) {
        if nf::is_neg(y) {
            base.sub(&pi_fx)
        } else {
            base.add(&pi_fx)
        }
    } else {
        base
    };
    nf::from_fx(&v, s, true)
}

/// `sinh(a)`; overflows to `+/-Inf`, `sinh(+/-Eps) = +/-Eps`.
pub fn sinh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_inf(a) || nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let s = scale();
    let f = nf::frac_bits();
    let x = nf::to_fx(a, GUARD);
    let lim = fx_ln2(s).mul_small(f as u64 + 3);
    if x.abs().cmp(&lim) != Ordering::Less {
        return nf::inf(x.neg);
    }
    let e = fx_exp(&x.abs(), s);
    let v = e.sub(&fx_one(s).div_s(&e, s)).shr(1);
    nf::from_fx(&if x.neg { v.neg() } else { v }, s, true)
}

/// `cosh(a)`; `cosh(+/-Inf) = +Inf`, `cosh(+/-Eps) = 1`,
/// `cosh(+/-Any) = +Any` (always `> 1` but unbounded).
pub fn cosh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_any(a) {
        return nf::any(false);
    }
    if nf::is_inf(a) {
        return nf::inf(false);
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return nf::one();
    }
    let s = scale();
    let f = nf::frac_bits();
    let x = nf::to_fx(a, GUARD).abs();
    let lim = fx_ln2(s).mul_small(f as u64 + 3);
    if x.cmp(&lim) != Ordering::Less {
        return nf::inf(false);
    }
    let e = fx_exp(&x, s);
    let v = e.add(&fx_one(s).div_s(&e, s)).shr(1);
    nf::from_fx(&v, s, true)
}

/// `tanh(a)`; `tanh(+/-Inf) = +/-1`, `tanh(+/-Eps) = +/-Eps`,
/// `tanh(+/-Any) = +/-Any` (in `(0,1)`-ish, sign known).
pub fn tanh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    if nf::is_inf(a) {
        return nf::with_sign(&nf::one(), nf::is_neg(a));
    }
    let s = scale();
    let f = nf::frac_bits();
    let x = nf::to_fx(a, GUARD);
    let lim = fx_ln2(s).mul_small(f as u64 + 3);
    if x.abs().cmp(&lim) != Ordering::Less {
        // 1 - tanh|x| < 2^-(2F): rounds to exactly +/-1
        return nf::with_sign(&nf::one(), x.neg);
    }
    let e2 = fx_exp(&x.abs().shl(1), s); // e^(2|x|)
    let one = fx_one(s);
    let v = e2.sub(&one).div_s(&e2.add(&one), s);
    nf::from_fx(&if x.neg { v.neg() } else { v }, s, true)
}

/// `asinh(a) = sign(a) * ln(|a| + sqrt(a^2+1))`.
pub fn asinh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) {
        return nf::nan();
    }
    if nf::is_inf(a) || nf::is_eps(a) || nf::is_any(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let s = scale();
    let x = nf::to_fx(a, GUARD).abs();
    let t = x.mul_rs(&x, s).add(&fx_one(s)).sqrt_s(s).add(&x);
    let v = fx_ln(&t, s);
    nf::from_fx(&if nf::is_neg(a) { v.neg() } else { v }, s, true)
}

/// `acosh(a) = ln(a + sqrt(a^2-1))`, domain `a >= 1`.
pub fn acosh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_any(a) {
        return nf::nan(); // Any: could be below the domain start
    }
    if nf::is_inf(a) {
        return if nf::is_neg(a) { nf::nan() } else { nf::inf(false) };
    }
    let one = nf::one();
    match nf::cmp(a, &one) {
        None => nf::nan(),
        Some(Ordering::Less) => nf::nan(),
        Some(Ordering::Equal) => nf::zero(),
        Some(Ordering::Greater) => {
            let s = scale();
            let x = nf::to_fx(a, GUARD);
            let t = x.mul_rs(&x, s).sub(&fx_one(s)).sqrt_s(s).add(&x);
            nf::from_fx(&fx_ln(&t, s), s, true)
        }
    }
}

/// `atanh(a)`: `atanh(+/-1) = +/-Inf`, `|a| > 1` gives NaN.
pub fn atanh(a: &[Limb]) -> Vec<Limb> {
    if nf::is_nan(a) || nf::is_inf(a) || nf::is_any(a) {
        return nf::nan(); // Any: could be outside (-1, 1)
    }
    if nf::is_eps(a) || nf::is_zero(a) {
        return a.to_vec();
    }
    let one = nf::one();
    match nf::cmp(&nf::abs(a), &one).unwrap() {
        Ordering::Greater => nf::nan(),
        Ordering::Equal => nf::inf(nf::is_neg(a)),
        Ordering::Less => {
            let s = scale();
            let x = nf::to_fx(a, GUARD);
            let one_fx = fx_one(s);
            let v = fx_ln(&one_fx.add(&x).div_s(&one_fx.sub(&x), s), s).shr(1);
            nf::from_fx(&v, s, true)
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nf::{with_n, from_str, to_string_digits};

    const TEST_NS: [u32; 6] = [1, 2, 3, 6, 7, 8];

    // 80-digit references
    const PI_80: &str =
        "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899";
    const E_80: &str =
        "2.71828182845904523536028747135266249775724709369995957496696762772407663035354759";
    const LN2_80: &str =
        "0.69314718055994530941723212145817656807550013436025525412068000949339362196969471";
    const SQRT2_80: &str =
        "1.41421356237309504880168872420969807856967187537694807317667973799073247846210703";

    fn p(x: &str) -> Vec<Limb> {
        from_str(x).unwrap()
    }

    /// |a - b| <= k ulp
    fn close(a: &[Limb], b: &[Limb], k: u64) -> bool {
        let d = nf::abs(&nf::sub(a, b));
        if nf::is_eps(&d) || nf::is_zero(&d) {
            return true;
        }
        if !nf::is_normal(&d) {
            return false;
        }
        let mut bound = nf::min_pos();
        bound = nf::mul(&bound, &nf::from_u64(k));
        nf::cmp(&d, &bound) != Some(Ordering::Greater)
    }

    #[test]
    fn constants_vs_references() {
        // N=7 -> F=128 (~38 digits), N=8 -> F=256 (~77 digits)
        for &n in &[7u32, 8] {
            with_n(n, || {
                assert!(close(&pi(), &p(PI_80), 1), "pi at n={}", n);
                assert!(close(&e_const(), &p(E_80), 1), "e at n={}", n);
                assert!(close(&ln2_const(), &p(LN2_80), 1), "ln2 at n={}", n);
                assert!(close(&sqrt(&p("2")), &p(SQRT2_80), 1), "sqrt2 at n={}", n);
            });
        }
        // spot-check printed digits at n=8 (77 digits are meaningful)
        with_n(8, || {
            let got = to_string_digits(&pi(), Some(70));
            assert_eq!(&got[..66], &PI_80[..66]);
        });
    }

    #[test]
    fn sqrt_exact_and_domain() {
        for &n in TEST_NS.iter() {
            with_n(n, || {
                if n >= 2 {
                    assert!(nf::eq(&sqrt(&p("4")), &p("2")), "n={}", n);
                }
                assert!(nf::eq(&sqrt(&p("2.25")), &p("1.5")), "n={}", n);
                assert!(nf::is_nan(&sqrt(&p("-1"))), "n={}", n);
                assert!(nf::is_zero(&sqrt(&nf::zero())), "n={}", n);
                assert!(nf::is_eps(&sqrt(&nf::eps(false))), "n={}", n);
                assert!(nf::is_nan(&sqrt(&nf::eps(true))), "n={}", n);
                assert!(nf::is_inf(&sqrt(&nf::inf(false))), "n={}", n);
                assert!(nf::is_nan(&sqrt(&nf::inf(true))), "n={}", n);
                // sqrt(x)^2 within 2 ulp-ish of x
                let r = sqrt(&p("2"));
                assert!(close(&nf::mul(&r, &r), &p("2"), 4), "n={}", n);
            });
        }
    }

    #[test]
    fn roots() {
        for &n in &[3u32, 6, 7, 8] {
            with_n(n, || {
                assert!(nf::eq(&nth_root(&p("8"), 3), &p("2")), "n={}", n);
                assert!(nf::eq(&nth_root(&p("-8"), 3), &p("-2")), "n={}", n);
                assert!(nf::is_nan(&nth_root(&p("-4"), 2)), "n={}", n);
                assert!(nf::is_nan(&nth_root(&p("5"), 0)), "n={}", n);
                assert!(nf::eq(&nth_root(&p("5"), 1), &p("5")), "n={}", n);
                let r = nth_root(&p("2"), 5);
                assert!(close(&pow(&r, &p("5")), &p("2"), 8), "n={}", n);
                // huge-k path
                let rk = nth_root(&p("2"), 1000);
                let lo = p("1");
                assert!(nf::cmp(&rk, &lo) != Some(Ordering::Less), "n={}", n);
            });
        }
    }

    #[test]
    fn exp_ln_identities() {
        for &n in TEST_NS.iter() {
            with_n(n, || {
                assert!(nf::eq(&exp(&nf::zero()), &nf::one()), "n={}", n);
                assert!(nf::eq(&exp(&nf::eps(true)), &nf::one()), "n={}", n);
                assert!(nf::is_inf(&exp(&nf::inf(false))), "n={}", n);
                assert!(nf::is_eps(&exp(&nf::inf(true))), "n={}", n);
                assert!(nf::is_zero(&ln(&nf::one())), "n={}", n);
                assert!(nf::is_nan(&ln(&p("-2"))), "n={}", n);
                assert!(nf::is_inf(&ln(&nf::zero())) && nf::is_neg(&ln(&nf::zero())), "n={}", n);
                assert!(nf::is_inf(&ln(&nf::eps(false))) && nf::is_neg(&ln(&nf::eps(false))), "n={}", n);
                assert!(nf::is_nan(&ln(&nf::eps(true))), "n={}", n);
                assert!(nf::is_inf(&ln(&nf::inf(false))), "n={}", n);
                if n >= 3 {
                    // exp(ln 5) ~ 5 and ln(exp 2) ~ 2 within a few ulp
                    assert!(close(&exp(&ln(&p("5"))), &p("5"), 6), "n={}", n);
                    assert!(close(&ln(&exp(&p("2"))), &p("2"), 6), "n={}", n);
                    assert!(close(&exp(&nf::one()), &e_const(), 2), "n={}", n);
                }
                // overflow / underflow saturation
                assert!(nf::is_inf(&exp(&nf::max_value())), "n={}", n);
                assert!(nf::is_eps(&exp(&nf::neg(&nf::max_value()))), "n={}", n);
            });
        }
    }

    #[test]
    fn logs() {
        with_n(7, || {
            assert!(nf::eq(&log2(&p("8")), &p("3")));
            assert!(nf::eq(&log2(&p("0.25")), &p("-2")));
            assert!(nf::eq(&log10(&p("1000")), &p("3")));
            assert!(close(&logb(&p("3"), &p("81")), &p("4"), 2));
            assert!(nf::is_nan(&log2(&p("-1"))));
            assert!(nf::is_inf(&log10(&nf::zero())));
        });
    }

    #[test]
    fn pow_table() {
        for &n in &[2u32, 3, 6, 7, 8] {
            with_n(n, || {
                let two = p("2");
                assert!(nf::eq(&pow(&two, &p("3")), &p("8")), "n={}", n);
                assert!(nf::eq(&pow(&p("-2"), &p("3")), &p("-8")), "n={}", n);
                assert!(nf::eq(&pow(&p("-2"), &p("2")), &p("4")), "n={}", n);
                assert!(nf::eq(&pow(&two, &p("-1")), &p("0.5")), "n={}", n);
                assert!(nf::eq(&pow(&two, &nf::zero()), &nf::one()), "n={}", n);
                assert!(nf::eq(&pow(&nf::zero(), &nf::zero()), &nf::one()), "n={}", n);
                assert!(nf::is_zero(&pow(&nf::zero(), &two)), "n={}", n);
                assert!(nf::is_nan(&pow(&nf::zero(), &p("-2"))), "n={}", n);
                assert!(nf::is_nan(&pow(&p("-2"), &p("0.5"))), "n={}", n);
                assert!(nf::is_nan(&pow(&nf::one(), &nf::inf(false))), "n={}", n);
                assert!(nf::eq(&pow(&nf::inf(true), &nf::zero()), &nf::one()), "n={}", n);
                assert!(nf::is_inf(&pow(&two, &nf::inf(false))), "n={}", n);
                assert!(nf::is_eps(&pow(&two, &nf::inf(true))), "n={}", n);
                assert!(nf::is_eps(&pow(&p("0.5"), &nf::inf(false))), "n={}", n);
                assert!(nf::is_inf(&pow(&p("0.5"), &nf::inf(true))), "n={}", n);
                assert!(nf::is_nan(&pow(&p("-2"), &nf::inf(false))), "n={}", n);
                // Inf/Eps bases with integral exponents keep parity signs
                assert!(nf::is_inf(&pow(&nf::inf(true), &p("3"))) && nf::is_neg(&pow(&nf::inf(true), &p("3"))), "n={}", n);
                assert!(nf::is_eps(&pow(&nf::inf(true), &p("-3"))) && nf::is_neg(&pow(&nf::inf(true), &p("-3"))), "n={}", n);
                assert!(nf::is_eps(&pow(&nf::eps(true), &p("3"))) && nf::is_neg(&pow(&nf::eps(true), &p("3"))), "n={}", n);
                if n >= 6 {
                    assert!(close(&pow(&two, &p("0.5")), &sqrt(&two), 2), "n={}", n);
                }
                // saturation on huge exponents
                assert!(nf::is_inf(&pow(&two, &nf::max_value())), "n={}", n);
                assert!(nf::is_eps(&pow(&two, &nf::neg(&nf::max_value()))), "n={}", n);
            });
        }
    }

    #[test]
    fn trig_identities() {
        for &n in TEST_NS.iter() {
            with_n(n, || {
                assert!(nf::is_nan(&sin(&nf::inf(false))), "n={}", n);
                assert!(nf::is_eps(&sin(&nf::eps(false))), "n={}", n);
                assert!(nf::eq(&cos(&nf::eps(true)), &nf::one()), "n={}", n);
                assert!(nf::is_zero(&sin(&nf::zero())), "n={}", n);
                assert!(nf::eq(&cos(&nf::zero()), &nf::one()), "n={}", n);
                let x = p("0.5");
                let (sn, cs) = (sin(&x), cos(&x));
                let ss = nf::add(&nf::mul(&sn, &sn), &nf::mul(&cs, &cs));
                assert!(close(&ss, &nf::one(), 4), "n={} sin2+cos2", n);
                if n >= 6 {
                    // sin(pi) is below one ulp -> Eps or ~0
                    let sp = sin(&pi());
                    assert!(nf::is_eps(&sp) || close(&sp, &nf::zero(), 1), "n={}", n);
                    assert!(close(&cos(&pi()), &p("-1"), 1), "n={}", n);
                    // tan(pi/4) ~ 1, atan(1)*4 ~ pi
                    let quarter = nf::div(&pi(), &p("4"));
                    assert!(close(&tan(&quarter), &nf::one(), 4), "n={}", n);
                    assert!(close(&nf::mul(&atan(&nf::one()), &p("4")), &pi(), 4), "n={}", n);
                    // large-argument reduction: sin(1000) via identity sin^2+cos^2
                    let big = p("1000");
                    let (s2, c2) = (sin(&big), cos(&big));
                    let one_chk = nf::add(&nf::mul(&s2, &s2), &nf::mul(&c2, &c2));
                    assert!(close(&one_chk, &nf::one(), 4), "n={} big arg", n);
                }
            });
        }
    }

    #[test]
    fn inverse_trig() {
        for &n in &[6u32, 7, 8] {
            with_n(n, || {
                let half = p("0.5");
                assert!(close(&sin(&asin(&half)), &half, 4), "n={}", n);
                assert!(close(&cos(&acos(&half)), &half, 4), "n={}", n);
                assert!(nf::is_nan(&asin(&p("1.5"))), "n={}", n);
                assert!(nf::is_nan(&acos(&p("-1.5"))), "n={}", n);
                // asin(1) = pi/2
                let hp = nf::div(&pi(), &p("2"));
                assert!(close(&asin(&nf::one()), &hp, 2), "n={}", n);
                assert!(close(&acos(&p("-1")), &pi(), 2), "n={}", n);
                assert!(nf::is_zero(&acos(&nf::one())), "n={}", n);
                assert!(close(&atan(&nf::inf(false)), &hp, 2), "n={}", n);
                // atan2 quadrants
                let one = nf::one();
                let m1 = p("-1");
                let q1 = atan2(&one, &one);
                let q2 = atan2(&one, &m1);
                let q3 = atan2(&m1, &m1);
                let q4 = atan2(&m1, &one);
                let quarter = nf::div(&pi(), &p("4"));
                assert!(close(&q1, &quarter, 2), "n={}", n);
                assert!(close(&q2, &nf::mul(&quarter, &p("3")), 2), "n={}", n);
                assert!(close(&q3, &nf::mul(&quarter, &p("-3")), 2), "n={}", n);
                assert!(close(&q4, &nf::neg(&quarter), 2), "n={}", n);
                assert!(nf::is_nan(&atan2(&nf::zero(), &nf::zero())), "n={}", n);
                assert!(close(&atan2(&nf::zero(), &m1), &pi(), 2), "n={}", n);
                assert!(nf::is_zero(&atan2(&nf::zero(), &one)), "n={}", n);
            });
        }
    }

    #[test]
    fn hyperbolics() {
        for &n in &[3u32, 6, 7, 8] {
            with_n(n, || {
                let x = p("0.5");
                // cosh^2 - sinh^2 = 1
                let (sh, ch) = (sinh(&x), cosh(&x));
                let idn = nf::sub(&nf::mul(&ch, &ch), &nf::mul(&sh, &sh));
                assert!(close(&idn, &nf::one(), 6), "n={}", n);
                assert!(close(&tanh(&x), &nf::div(&sh, &ch), 4), "n={}", n);
                assert!(close(&asinh(&sh), &x, 6), "n={}", n);
                assert!(close(&acosh(&ch), &x, 8), "n={}", n);
                assert!(close(&atanh(&tanh(&x)), &x, 6), "n={}", n);
                // specials
                assert!(nf::is_inf(&sinh(&nf::inf(true))) && nf::is_neg(&sinh(&nf::inf(true))), "n={}", n);
                assert!(nf::is_inf(&cosh(&nf::inf(true))) && !nf::is_neg(&cosh(&nf::inf(true))), "n={}", n);
                assert!(nf::eq(&tanh(&nf::inf(true)), &p("-1")), "n={}", n);
                assert!(nf::is_eps(&tanh(&nf::eps(false))), "n={}", n);
                assert!(nf::is_nan(&acosh(&p("0.5"))), "n={}", n);
                assert!(nf::is_inf(&atanh(&p("-1"))) && nf::is_neg(&atanh(&p("-1"))), "n={}", n);
                assert!(nf::is_nan(&atanh(&p("2"))), "n={}", n);
                // saturation
                assert!(nf::is_inf(&sinh(&nf::max_value())), "n={}", n);
                assert!(nf::eq(&tanh(&nf::max_value()), &nf::one()), "n={}", n);
            });
        }
    }
}

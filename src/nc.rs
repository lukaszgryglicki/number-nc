//! NC - complex numbers built from two NF numbers.
//!
//! An NC number is simply two NF word arrays back to back in one `Vec<u64>`:
//! `[ re words..., im words... ]` (each part is `nf::words()` machine words:
//! mantissa limbs + flags word).  Like NF, the representation is raw machine
//! words with no stored `N` - the global [`nf::set_n`] applies.
//!
//! Accuracy: `mul`, `div` and `abs` of normal values are computed **exactly**
//! at the mantissa level and rounded once (ties away from zero).  The
//! transcendental functions compose correctly-rounded real functions, so
//! their components are accurate to a few ulps.  Operations involving
//! special values (NaN / +-Inf / +-InfSmall) fall back to component formulas
//! whose NF special algebra decides the result; indeterminate combinations
//! yield NaN components.

use crate::fx::Fx;
use crate::limb;
use crate::limb::Limb;
use crate::nf;
use crate::real;
use std::cmp::Ordering;

// ---------------------------------------------------------------------------
// construction / access
// ---------------------------------------------------------------------------

/// Build an NC number from real and imaginary NF parts.
pub fn make(re: &[Limb], im: &[Limb]) -> Vec<Limb> {
    let w = nf::words();
    debug_assert_eq!(re.len(), w);
    debug_assert_eq!(im.len(), w);
    let mut v = Vec::with_capacity(2 * w);
    v.extend_from_slice(re);
    v.extend_from_slice(im);
    v
}

/// Real part (borrow).
pub fn re(z: &[Limb]) -> &[Limb] {
    &z[..z.len() / 2]
}

/// Imaginary part (borrow).
pub fn im(z: &[Limb]) -> &[Limb] {
    &z[z.len() / 2..]
}

/// `0 + 0i`.
pub fn zero() -> Vec<Limb> {
    make(&nf::zero(), &nf::zero())
}

/// `1 + 0i`.
pub fn one() -> Vec<Limb> {
    make(&nf::one(), &nf::zero())
}

/// The imaginary unit `i`.
pub fn i_unit() -> Vec<Limb> {
    make(&nf::zero(), &nf::one())
}

/// `NaN + NaN i`.
pub fn nan() -> Vec<Limb> {
    make(&nf::nan(), &nf::nan())
}

/// Purely real complex from an NF value.
pub fn from_re(re_: &[Limb]) -> Vec<Limb> {
    make(re_, &nf::zero())
}

/// Either component is NaN.
pub fn is_nan(z: &[Limb]) -> bool {
    nf::is_nan(re(z)) || nf::is_nan(im(z))
}

/// Both components are exactly zero.
pub fn is_zero(z: &[Limb]) -> bool {
    nf::is_zero(re(z)) && nf::is_zero(im(z))
}

/// Both components are normal (no special flags; zero allowed).
pub fn is_all_normal(z: &[Limb]) -> bool {
    nf::is_normal(re(z)) && nf::is_normal(im(z))
}

/// Equality: both components equal (NaN never equals anything).
pub fn eq(a: &[Limb], b: &[Limb]) -> bool {
    nf::eq(re(a), re(b)) && nf::eq(im(a), im(b))
}

// ---------------------------------------------------------------------------
// elementary operations
// ---------------------------------------------------------------------------

/// `a + b`.
pub fn add(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    make(&nf::add(re(a), re(b)), &nf::add(im(a), im(b)))
}

/// `a - b`.
pub fn sub(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    make(&nf::sub(re(a), re(b)), &nf::sub(im(a), im(b)))
}

/// `-a`.
pub fn neg(a: &[Limb]) -> Vec<Limb> {
    make(&nf::neg(re(a)), &nf::neg(im(a)))
}

/// Complex conjugate.
pub fn conj(a: &[Limb]) -> Vec<Limb> {
    make(re(a), &nf::neg(im(a)))
}

/// `a * i` (rotate by 90 degrees) - exact.
pub fn mul_i(a: &[Limb]) -> Vec<Limb> {
    make(&nf::neg(im(a)), re(a))
}

/// `a * (-i)` - exact.
pub fn mul_neg_i(a: &[Limb]) -> Vec<Limb> {
    make(im(a), &nf::neg(re(a)))
}

/// Signed exact product of two NF mantissas as an [`Fx`] at scale `2F`.
fn cross(x: &[Limb], y: &[Limb]) -> Fx {
    Fx::new(
        nf::is_neg(x) != nf::is_neg(y),
        limb::mul(nf::mant(x), nf::mant(y)),
    )
}

/// `a * b`.
///
/// For all-normal operands: `(ac - bd) + (ad + bc)i` with the sums computed
/// exactly at scale `2F` and rounded once per component.
pub fn mul(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    let (ar, ai, br, bi) = (re(a), im(a), re(b), im(b));
    if is_all_normal(a) && is_all_normal(b) {
        let f = nf::frac_bits();
        let re_x = cross(ar, br).sub(&cross(ai, bi));
        let im_x = cross(ar, bi).add(&cross(ai, br));
        let (rn, rm) = (re_x.neg, limb::round_shr(&re_x.mag, f));
        let (in_, imm) = (im_x.neg, limb::round_shr(&im_x.mag, f));
        return make(
            &nf::pack(rn, &rm, !re_x.is_zero()),
            &nf::pack(in_, &imm, !im_x.is_zero()),
        );
    }
    // special-value path: NaN poisons everything; an exact complex zero
    // annihilates finite operands but 0 * (Inf or Any) is indeterminate;
    // otherwise a term with an exact-zero factor vanishes structurally (the
    // operand has no such part), so a zero component never forms 0*Inf with
    // the other operand.
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    let (a_zero, b_zero) = (is_zero(a), is_zero(b));
    if a_zero || b_zero {
        let o = if a_zero { b } else { a };
        let unbounded = |v: &[Limb]| nf::is_inf(v) || nf::is_any(v);
        return if unbounded(re(o)) || unbounded(im(o)) { nan() } else { zero() };
    }
    let term = |x: &[Limb], y: &[Limb]| -> Vec<Limb> {
        if nf::is_zero(x) || nf::is_zero(y) {
            nf::zero()
        } else {
            nf::mul(x, y)
        }
    };
    let rr = nf::sub(&term(ar, br), &term(ai, bi));
    let ii = nf::add(&term(ar, bi), &term(ai, br));
    make(&rr, &ii)
}

/// `a / b`.
///
/// For all-normal operands the two components are computed as exact big
/// rationals `(ac+bd)/(c^2+d^2)` and `(bc-ad)/(c^2+d^2)` and rounded once.
/// Division by exact complex zero gives NaN.
pub fn div(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    let (ar, ai, br, bi) = (re(a), im(a), re(b), im(b));
    if is_all_normal(a) && is_all_normal(b) {
        if is_zero(b) {
            return nan();
        }
        let f = nf::frac_bits();
        // den = c^2 + d^2 at scale 2^-2F (exact, positive)
        let den = limb::add(
            &limb::mul(nf::mant(br), nf::mant(br)),
            &limb::mul(nf::mant(bi), nf::mant(bi)),
        );
        let re_num = cross(ar, br).add(&cross(ai, bi));
        let im_num = cross(ai, br).sub(&cross(ar, bi));
        let quot = |num: &Fx| -> Vec<Limb> {
            if num.is_zero() {
                return nf::zero();
            }
            // round(num * 2^F / den) with one extra bit for the tie
            let q = limb::div_rem(&limb::shl(&num.mag, f + 1), &den).0;
            let m = limb::round_shr(&q, 1);
            nf::pack(num.neg, &m, true)
        };
        return make(&quot(&re_num), &quot(&im_num));
    }
    // special-value path: Smith's algorithm on NF components.  A product
    // with an exact-zero factor vanishes structurally (see mul above), so
    // zeros never form 0*Inf / 0*Any with the other operand.
    if is_nan(a) || is_nan(b) {
        return nan();
    }
    let term = |x: &[Limb], y: &[Limb]| -> Vec<Limb> {
        if nf::is_zero(x) || nf::is_zero(y) {
            nf::zero()
        } else {
            nf::mul(x, y)
        }
    };
    let (c_abs, d_abs) = (nf::abs(br), nf::abs(bi));
    if nf::cmp(&c_abs, &d_abs) != Some(Ordering::Less) {
        let r = nf::div(bi, br);
        let den = nf::add(br, &term(bi, &r));
        let rr = nf::div(&nf::add(ar, &term(ai, &r)), &den);
        let ii = nf::div(&nf::sub(ai, &term(ar, &r)), &den);
        make(&rr, &ii)
    } else {
        let r = nf::div(br, bi);
        let den = nf::add(&term(br, &r), bi);
        let rr = nf::div(&nf::add(&term(ar, &r), ai), &den);
        let ii = nf::div(&nf::sub(&term(ai, &r), ar), &den);
        make(&rr, &ii)
    }
}

/// `|a|` - the modulus, as an NF number.  Exact (single rounding) for normal
/// components: `round(sqrt(ma^2 + mb^2))` by integer square root.
pub fn abs(z: &[Limb]) -> Vec<Limb> {
    let (r, i) = (re(z), im(z));
    if nf::is_nan(r) || nf::is_nan(i) {
        return nf::nan();
    }
    if nf::is_inf(r) || nf::is_inf(i) {
        return nf::inf(false);
    }
    // an Any component makes the magnitude indeterminate (but positive)
    if nf::is_any(r) || nf::is_any(i) {
        return nf::any(false);
    }
    let re_eps = nf::is_eps(r);
    let im_eps = nf::is_eps(i);
    let re_zero = nf::is_zero(r);
    let im_zero = nf::is_zero(i);
    if (re_eps || re_zero) && (im_eps || im_zero) {
        return if re_eps || im_eps { nf::eps(false) } else { nf::zero() };
    }
    // exactly one component is a normal non-zero: |z| = |that component|
    // (exact, and avoids a huge isqrt when the other part is 0/Eps)
    if re_eps || re_zero {
        return nf::abs(i);
    }
    if im_eps || im_zero {
        return nf::abs(r);
    }
    // any Eps component is below one ulp: |z| rounds to |normal component|
    let (mr, mi) = (
        if re_eps { vec![] } else { nf::mant(r).to_vec() },
        if im_eps { vec![] } else { nf::mant(i).to_vec() },
    );
    let h = limb::isqrt_round(&limb::add(&limb::mul(&mr, &mr), &limb::mul(&mi, &mi)));
    nf::pack(false, &h, true)
}

/// `arg(z)` - the phase angle in `(-pi, pi]`; `arg(0)` is NaN.
pub fn arg(z: &[Limb]) -> Vec<Limb> {
    real::atan2(im(z), re(z))
}

// ---------------------------------------------------------------------------
// exponential family
// ---------------------------------------------------------------------------

/// `exp(z) = e^re * (cos im + i sin im)`.
pub fn exp(z: &[Limb]) -> Vec<Limb> {
    let er = real::exp(re(z));
    let (c, s) = (real::cos(im(z)), real::sin(im(z)));
    // sin/cos are exactly zero only for an exactly-zero angle, so that
    // component of the result is structurally zero (never 0*Inf, 0*Any)
    let rr = if nf::is_zero(&c) { nf::zero() } else { nf::mul(&er, &c) };
    let ii = if nf::is_zero(&s) { nf::zero() } else { nf::mul(&er, &s) };
    make(&rr, &ii)
}

/// Principal `ln(z) = ln|z| + i arg(z)`; `ln(0) = (-Inf) + NaN i`.
pub fn ln(z: &[Limb]) -> Vec<Limb> {
    make(&real::ln(&abs(z)), &arg(z))
}

/// Principal square root.
pub fn sqrt(z: &[Limb]) -> Vec<Limb> {
    if is_nan(z) {
        return nan();
    }
    let (a, b) = (re(z), im(z));
    if nf::is_zero(b) {
        // purely real: principal branch, +i side for negative reals
        return if nf::is_neg(a) && !nf::is_zero(a) {
            make(&nf::zero(), &real::sqrt(&nf::abs(a)))
        } else {
            make(&real::sqrt(a), &nf::zero())
        };
    }
    if !is_all_normal(z) {
        // compose via exp(ln(z)/2) so the NF special algebra decides
        let h = ln(z);
        let half = make(&nf::from_str("0.5").unwrap(), &nf::zero());
        return exp(&mul(&h, &half));
    }
    // stable formula with the exact hypot:
    //   t = sqrt((|z| + |a|)/2)
    //   a >= 0: (t, b/2t);  a < 0: (|b|/2t, sign(b)*t)
    let r = abs(z);
    let two = nf::from_u64(2);
    let t = real::sqrt(&nf::div(&nf::add(&r, &nf::abs(a)), &two));
    if !nf::is_neg(a) {
        let bi = nf::div(b, &nf::mul(&two, &t));
        make(&t, &bi)
    } else {
        let rr = nf::div(&nf::abs(b), &nf::mul(&two, &t));
        make(&rr, &nf::with_sign(&t, nf::is_neg(b)))
    }
}

/// `z^w` - full complex power.
pub fn pow(z: &[Limb], w: &[Limb]) -> Vec<Limb> {
    if is_nan(z) || is_nan(w) {
        return nan();
    }
    // w == 0 -> 1 (0^0 = 1 by convention, matching the real pow)
    if is_zero(w) {
        return one();
    }
    let w_re = re(w);
    let w_im = im(w);
    let w_is_real = nf::is_zero(w_im);
    // purely real, non-negative base with a purely real exponent: use the
    // real pow directly (exact special-value table, no complex detour);
    // this also covers every 0^w case with real w
    if w_is_real && nf::is_zero(im(z)) && !nf::is_neg(re(z)) {
        return from_re(&real::pow(re(z), w_re));
    }
    if is_zero(z) {
        return nan(); // 0^(complex w), w != 0
    }
    // real integral exponent: exact binary powering on complex mul
    if w_is_real && nf::is_integral(w_re) {
        if let Some(k) = small_abs_int(w_re) {
            let negative = nf::is_neg(w_re);
            let mut base = z.to_vec();
            let mut k = k;
            let mut acc: Option<Vec<Limb>> = None;
            loop {
                if k & 1 == 1 {
                    acc = Some(match acc {
                        None => base.clone(),
                        Some(a) => mul(&a, &base),
                    });
                }
                k >>= 1;
                if k == 0 {
                    break;
                }
                base = mul(&base, &base);
            }
            let v = acc.unwrap();
            return if negative { div(&one(), &v) } else { v };
        }
    }
    // general: exp(w * ln z)
    exp(&mul(w, &ln(z)))
}

fn small_abs_int(a: &[Limb]) -> Option<u64> {
    if !nf::is_integral(a) {
        return None;
    }
    let int = limb::shr(nf::mant(a), nf::frac_bits());
    if limb::bit_len(&int) > 62 {
        return None;
    }
    Some(*int.first().unwrap_or(&0))
}

/// Principal `k`-th root via `exp(ln(z)/k)`; for purely real non-negative
/// values (and odd `k` negatives) delegates to the exact real root.
pub fn root(z: &[Limb], k: u64) -> Vec<Limb> {
    if k == 0 || is_nan(z) {
        return nan();
    }
    if k == 1 {
        return z.to_vec();
    }
    let (a, b) = (re(z), im(z));
    if nf::is_zero(b) && (!nf::is_neg(a) || k % 2 == 1) {
        // note: for negative reals this is the *real* odd root, which is the
        // conventional calculator behaviour (not the principal complex root)
        return from_re(&real::nth_root(a, k));
    }
    if is_zero(z) {
        return zero();
    }
    let l = ln(z);
    let kk = from_re(&nf::from_u64(k));
    exp(&div(&l, &kk))
}

// ---------------------------------------------------------------------------
// trigonometric / hyperbolic family
// ---------------------------------------------------------------------------

/// `sin(z) = sin a cosh b + i cos a sinh b`.
pub fn sin(z: &[Limb]) -> Vec<Limb> {
    let (a, b) = (re(z), im(z));
    make(
        &nf::mul(&real::sin(a), &real::cosh(b)),
        &nf::mul(&real::cos(a), &real::sinh(b)),
    )
}

/// `cos(z) = cos a cosh b - i sin a sinh b`.
pub fn cos(z: &[Limb]) -> Vec<Limb> {
    let (a, b) = (re(z), im(z));
    make(
        &nf::mul(&real::cos(a), &real::cosh(b)),
        &nf::neg(&nf::mul(&real::sin(a), &real::sinh(b))),
    )
}

/// `tan(z) = sin(z)/cos(z)`.
pub fn tan(z: &[Limb]) -> Vec<Limb> {
    div(&sin(z), &cos(z))
}

/// `sinh(z) = sinh a cos b + i cosh a sin b`.
pub fn sinh(z: &[Limb]) -> Vec<Limb> {
    let (a, b) = (re(z), im(z));
    make(
        &nf::mul(&real::sinh(a), &real::cos(b)),
        &nf::mul(&real::cosh(a), &real::sin(b)),
    )
}

/// `cosh(z) = cosh a cos b + i sinh a sin b`.
pub fn cosh(z: &[Limb]) -> Vec<Limb> {
    let (a, b) = (re(z), im(z));
    make(
        &nf::mul(&real::cosh(a), &real::cos(b)),
        &nf::mul(&real::sinh(a), &real::sin(b)),
    )
}

/// `tanh(z) = sinh(z)/cosh(z)`.
pub fn tanh(z: &[Limb]) -> Vec<Limb> {
    div(&sinh(z), &cosh(z))
}

/// Principal `asin(z) = -i ln(iz + sqrt(1 - z^2))`.
pub fn asin(z: &[Limb]) -> Vec<Limb> {
    // purely real in [-1,1]: use the real function (better branch behaviour)
    if nf::is_zero(im(z)) && nf::is_normal(re(z)) {
        let r = real::asin(re(z));
        if !nf::is_nan(&r) {
            return from_re(&r);
        }
    }
    let one_ = one();
    let s = sqrt(&sub(&one_, &mul(z, z)));
    mul_neg_i(&ln(&add(&mul_i(z), &s)))
}

/// Principal `acos(z) = pi/2 - asin(z)`.
pub fn acos(z: &[Limb]) -> Vec<Limb> {
    if nf::is_zero(im(z)) && nf::is_normal(re(z)) {
        let r = real::acos(re(z));
        if !nf::is_nan(&r) {
            return from_re(&r);
        }
    }
    let half_pi = from_re(&nf::div(&real::pi(), &nf::from_u64(2)));
    sub(&half_pi, &asin(z))
}

/// Principal `atan(z) = -(i/2) ln((1+iz)/(1-iz))`.
pub fn atan(z: &[Limb]) -> Vec<Limb> {
    if nf::is_zero(im(z)) {
        return from_re(&real::atan(re(z)));
    }
    let one_ = one();
    let iz = mul_i(z);
    let q = div(&add(&one_, &iz), &sub(&one_, &iz));
    let l = ln(&q);
    let half = from_re(&nf::from_str("0.5").unwrap());
    mul_neg_i(&mul(&l, &half))
}

/// Principal `asinh(z) = ln(z + sqrt(z^2 + 1))`.
pub fn asinh(z: &[Limb]) -> Vec<Limb> {
    if nf::is_zero(im(z)) {
        return from_re(&real::asinh(re(z)));
    }
    ln(&add(z, &sqrt(&add(&mul(z, z), &one()))))
}

/// Principal `acosh(z) = ln(z + sqrt(z+1) sqrt(z-1))`.
pub fn acosh(z: &[Limb]) -> Vec<Limb> {
    if nf::is_zero(im(z)) && nf::is_normal(re(z)) {
        let r = real::acosh(re(z));
        if !nf::is_nan(&r) {
            return from_re(&r);
        }
    }
    let one_ = one();
    let s = mul(&sqrt(&add(z, &one_)), &sqrt(&sub(z, &one_)));
    ln(&add(z, &s))
}

/// Principal `atanh(z) = ln((1+z)/(1-z)) / 2`.
pub fn atanh(z: &[Limb]) -> Vec<Limb> {
    if nf::is_zero(im(z)) {
        return from_re(&real::atanh(re(z)));
    }
    let one_ = one();
    let q = div(&add(&one_, z), &sub(&one_, z));
    let half = from_re(&nf::from_str("0.5").unwrap());
    mul(&ln(&q), &half)
}

// ---------------------------------------------------------------------------
// componentwise integral helpers
// ---------------------------------------------------------------------------

/// Componentwise floor.
pub fn floor(z: &[Limb]) -> Vec<Limb> {
    make(&nf::floor(re(z)), &nf::floor(im(z)))
}

/// Componentwise ceil.
pub fn ceil(z: &[Limb]) -> Vec<Limb> {
    make(&nf::ceil(re(z)), &nf::ceil(im(z)))
}

/// Componentwise round (ties away from zero).
pub fn round(z: &[Limb]) -> Vec<Limb> {
    make(&nf::round(re(z)), &nf::round(im(z)))
}

/// Componentwise truncation.
pub fn trunc(z: &[Limb]) -> Vec<Limb> {
    make(&nf::trunc(re(z)), &nf::trunc(im(z)))
}

// ---------------------------------------------------------------------------
// formatting
// ---------------------------------------------------------------------------

fn wrap_component(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
        s.to_string()
    } else {
        format!("({})", s) // NaN / +Inf / +InfSmall
    }
}

/// Format as `a`, `bi`, or `a+bi` / `a-bi` with the requested fraction
/// digits per component (`None` = shortest roundtrip).
pub fn to_string_digits(z: &[Limb], digits: Option<usize>) -> String {
    let (r, i) = (re(z), im(z));
    if nf::is_nan(r) && nf::is_nan(i) {
        return wrap_component(&nf::to_string_digits(r, digits));
    }
    let re_str = wrap_component(&nf::to_string_digits(r, digits));
    if nf::is_zero(i) {
        return re_str;
    }
    let neg_im = nf::is_neg(i) && !nf::is_nan(i);
    let im_abs = nf::abs(i);
    let mut im_str = wrap_component(&nf::to_string_digits(&im_abs, digits));
    if im_str == "1" {
        im_str.clear(); // 1i -> i
    }
    if nf::is_zero(r) {
        return format!("{}{}{}i", if neg_im { "-" } else { "" }, im_str, "");
    }
    format!("{}{}{}i", re_str, if neg_im { "-" } else { "+" }, im_str)
}

/// Shortest-roundtrip formatting.
pub fn to_string(z: &[Limb]) -> String {
    to_string_digits(z, None)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nf::with_n;

    const TEST_NS: [u32; 6] = [1, 2, 3, 6, 7, 8];

    fn pr(x: &str) -> Vec<Limb> {
        nf::from_str(x).unwrap()
    }
    fn pc(re_: &str, im_: &str) -> Vec<Limb> {
        make(&pr(re_), &pr(im_))
    }

    /// component-wise closeness in ulps
    fn close(a: &[Limb], b: &[Limb], k: u64) -> bool {
        let cl = |x: &[Limb], y: &[Limb]| -> bool {
            let d = nf::abs(&nf::sub(x, y));
            if nf::is_eps(&d) || nf::is_zero(&d) {
                return true;
            }
            if !nf::is_normal(&d) {
                return false;
            }
            let bound = nf::mul(&nf::min_pos(), &nf::from_u64(k));
            nf::cmp(&d, &bound) != Some(Ordering::Greater)
        };
        cl(re(a), re(b)) && cl(im(a), im(b))
    }

    #[test]
    fn layout_and_formatting() {
        with_n(7, || {
            let z = pc("3.5", "-2.25");
            assert_eq!(re(&z).len(), nf::words());
            assert_eq!(to_string(&z), "3.5-2.25i");
            assert_eq!(to_string(&pc("0", "1")), "i");
            assert_eq!(to_string(&pc("0", "-1")), "-i");
            assert_eq!(to_string(&pc("2", "0")), "2");
            assert_eq!(to_string(&pc("0", "0")), "0");
            assert_eq!(to_string(&pc("0", "2.5")), "2.5i");
            assert_eq!(to_string(&pc("1", "1")), "1+i");
            assert_eq!(to_string(&make(&nf::inf(false), &pr("2"))), "(+Inf)+2i");
            assert_eq!(to_string(&make(&nf::nan(), &nf::zero())), "(NaN)");
        });
    }

    #[test]
    fn exact_mul_div() {
        for &n in TEST_NS.iter() {
            with_n(n, || {
                if n < 3 {
                    // small ranges: just verify closure on tiny values
                    let a = pc("1.25", "0.75");
                    let b = pc("0.5", "0.25");
                    let p = mul(&a, &b);
                    assert!(is_all_normal(&p) || !is_nan(&p), "n={}", n);
                    return;
                }
                // (1+2i)(3-i) = 5+5i exactly
                let p = mul(&pc("1", "2"), &pc("3", "-1"));
                assert!(eq(&p, &pc("5", "5")), "n={}", n);
                // division undoes it exactly
                let q = div(&p, &pc("1", "2"));
                assert!(eq(&q, &pc("3", "-1")), "n={}", n);
                // i^2 = -1
                assert!(eq(&mul(&i_unit(), &i_unit()), &pc("-1", "0")), "n={}", n);
                // conj/neg/mul_i are exact
                let z = pc("2.5", "-1.25");
                assert!(eq(&mul_i(&z), &pc("1.25", "2.5")), "n={}", n);
                assert!(eq(&mul_neg_i(&mul_i(&z)), &z), "n={}", n);
                assert!(eq(&conj(&conj(&z)), &z), "n={}", n);
                // division by zero -> NaN
                assert!(is_nan(&div(&pc("1", "1"), &zero())), "n={}", n);
            });
        }
    }

    #[test]
    fn abs_and_arg() {
        for &n in &[3u32, 6, 7, 8] {
            with_n(n, || {
                // |3+4i| = 5 exactly
                assert!(nf::eq(&abs(&pc("3", "4")), &pr("5")), "n={}", n);
                assert!(nf::is_zero(&abs(&zero())), "n={}", n);
                assert!(nf::is_inf(&abs(&make(&nf::inf(true), &pr("1")))), "n={}", n);
                assert!(nf::is_eps(&abs(&make(&nf::eps(true), &nf::zero()))), "n={}", n);
                // |a + eps i| = |a|
                assert!(nf::eq(&abs(&make(&pr("-2"), &nf::eps(false))), &pr("2")), "n={}", n);
                assert!(nf::is_nan(&arg(&zero())), "n={}", n);
                if n >= 6 {
                    let hp = nf::div(&real::pi(), &pr("2"));
                    assert!(nf::eq(&arg(&i_unit()), &hp) || {
                        let d = nf::abs(&nf::sub(&arg(&i_unit()), &hp));
                        nf::cmp(&d, &nf::min_pos()) != Some(Ordering::Greater)
                    }, "n={}", n);
                }
            });
        }
    }

    #[test]
    fn euler_identity() {
        for &n in &[6u32, 7, 8] {
            with_n(n, || {
                // exp(i*pi) = -1 (+ Eps-sized imaginary dust)
                let z = make(&nf::zero(), &real::pi());
                let e = exp(&z);
                assert!(nf::eq(re(&e), &pr("-1")), "n={} re={}", n, nf::to_string(re(&e)));
                let idust = im(&e);
                assert!(
                    nf::is_eps(idust)
                        || nf::cmp(&nf::abs(idust), &nf::min_pos()) != Some(Ordering::Greater),
                    "n={}",
                    n
                );
                // ln(-1) = i*pi
                let l = ln(&pc("-1", "0"));
                assert!(nf::is_zero(re(&l)) || nf::is_eps(&nf::abs(re(&l))), "n={}", n);
                assert!(close(&from_re(im(&l)), &from_re(&real::pi()), 2), "n={}", n);
            });
        }
    }

    #[test]
    fn sqrt_and_pow() {
        for &n in &[3u32, 6, 7, 8] {
            with_n(n, || {
                // sqrt(-1) = i
                assert!(eq(&sqrt(&pc("-1", "0")), &i_unit()), "n={}", n);
                // sqrt(2i) = 1+i exactly
                assert!(close(&sqrt(&pc("0", "2")), &pc("1", "1"), 2), "n={}", n);
                // (1+i)^8 = 16
                assert!(eq(&pow(&pc("1", "1"), &pc("8", "0")), &pc("16", "0")), "n={}", n);
                // z^-2 = 1/z^2
                let z = pc("1", "1");
                let inv2 = pow(&z, &pc("-2", "0"));
                assert!(close(&inv2, &pc("0", "-0.5"), 2), "n={}", n); // (1+i)^2=2i, 1/(2i)=-i/2
                // 0^0 = 1, 0^2 = 0, 0^-1 = NaN
                assert!(eq(&pow(&zero(), &zero()), &one()), "n={}", n);
                assert!(is_zero(&pow(&zero(), &pc("2", "0"))), "n={}", n);
                assert!(is_nan(&pow(&zero(), &pc("-1", "0"))), "n={}", n);
                if n >= 6 {
                    // sqrt(z)^2 = z for a generic z
                    let w = pc("1.5", "-2.25");
                    let s = sqrt(&w);
                    assert!(close(&mul(&s, &s), &w, 6), "n={}", n);
                    // i^i = exp(-pi/2) (purely real)
                    let ii = pow(&i_unit(), &i_unit());
                    let want = real::exp(&nf::neg(&nf::div(&real::pi(), &pr("2"))));
                    assert!(close(&ii, &from_re(&want), 4), "n={}", n);
                    // root(z, 3)^3 = z
                    let r3 = root(&w, 3);
                    assert!(close(&pow(&r3, &pc("3", "0")), &w, 16), "n={}", n);
                    // real odd root of a negative real stays real
                    assert!(eq(&root(&pc("-8", "0"), 3), &pc("-2", "0")), "n={}", n);
                }
            });
        }
    }

    #[test]
    fn complex_trig() {
        for &n in &[6u32, 7, 8] {
            with_n(n, || {
                let z = pc("0.5", "0.3");
                // sin^2 + cos^2 = 1
                let (s, c) = (sin(&z), cos(&z));
                let idn = add(&mul(&s, &s), &mul(&c, &c));
                assert!(close(&idn, &one(), 8), "n={}", n);
                // cosh^2 - sinh^2 = 1
                let (sh, ch) = (sinh(&z), cosh(&z));
                let idn2 = sub(&mul(&ch, &ch), &mul(&sh, &sh));
                assert!(close(&idn2, &one(), 8), "n={}", n);
                // sin(iz) = i sinh(z)
                assert!(close(&sin(&mul_i(&z)), &mul_i(&sinh(&z)), 8), "n={}", n);
                // tan = sin/cos
                assert!(close(&tan(&z), &div(&s, &c), 8), "n={}", n);
                // inverse roundtrips
                assert!(close(&sin(&asin(&z)), &z, 32), "n={}", n);
                assert!(close(&cos(&acos(&z)), &z, 32), "n={}", n);
                assert!(close(&tan(&atan(&z)), &z, 32), "n={}", n);
                assert!(close(&sinh(&asinh(&z)), &z, 32), "n={}", n);
                assert!(close(&tanh(&atanh(&z)), &z, 32), "n={}", n);
                let zc = pc("1.5", "0.5");
                assert!(close(&cosh(&acosh(&zc)), &zc, 32), "n={}", n);
                // purely real asin stays real
                let r = asin(&pc("0.5", "0"));
                assert!(nf::is_zero(im(&r)), "n={}", n);
            });
        }
    }

    #[test]
    fn componentwise_integrals() {
        with_n(7, || {
            let z = pc("2.7", "-1.2");
            assert!(eq(&floor(&z), &pc("2", "-2")));
            assert!(eq(&ceil(&z), &pc("3", "-1")));
            assert!(eq(&trunc(&z), &pc("2", "-1")));
            assert!(eq(&round(&pc("2.5", "-2.5")), &pc("3", "-3")));
        });
    }
}

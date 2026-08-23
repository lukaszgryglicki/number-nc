//! Exhaustive 0F and 1F operation checks, bit-exact.
//!
//! A 0F number is 6 bits ([NaN][Inf][InfSmall][Sign][i][f]) and a 1F number
//! is 8 bits ([NaN][Inf][InfSmall][Sign][ii][ff]), so ALL raw encodings can
//! be enumerated — including every non-canonical one (NaN+Inf+InfSmall at
//! once, specials with payload bits, -0, ...).  These suites feed every
//! ordered pair of raw encodings to each of the five binary operations
//! add / subtract / multiply / divide / pow:
//!
//!   0F:  64 x  64 = 4096  pairs x 5 ops =  20 480 cases
//!   1F: 256 x 256 = 65536 pairs x 5 ops = 327 680 cases
//!
//! and require the full output encoding to match, bit for bit, an
//! independent exact oracle implemented straight from the README operation
//! matrices: small-integer rational arithmetic in units of 2^-F,
//! round-to-nearest ties-away-from-zero (pow decided by exact u128
//! power comparisons — no floating point), overflow -> Inf,
//! underflow -> InfSmall.
//!
//! Heavy and deliberately opt-in:
//!   `make test ALL0F=1` (env `NF_ALL0F`) runs the 0F suite,
//!   `make test ALL1F=1` (env `NF_ALL1F`) runs the 1F suite.
//! Without the variables the tests are no-ops.

use nf::nf::{self as nfm, with_n};
use nf::{real, Limb};

fn enabled(var: &str) -> bool {
    std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false)
}

/// Semantic value of an NF number.  `Num(q)` holds units of `2^-F`
/// (halves at 0F, quarters at 1F); zero is unsigned (q = 0).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Val {
    Nan,
    Inf(bool), // true = negative
    Any(bool),
    Eps(bool),
    Num(i64),
}
use Val::*;

/// Raw little-integer encoding -> bit string "ffff-int-frac"
/// (the most significant of the 2F+4 bits is NaN, the least is frac LSB).
fn bits_str(p: u32, f: usize) -> String {
    let tb = 2 * f + 4;
    let bit = |k: usize| if p >> (tb - 1 - k) & 1 == 1 { '1' } else { '0' };
    let mut out = String::new();
    for k in 0..4 {
        out.push(bit(k));
    }
    out.push('-');
    for k in 0..f {
        out.push(bit(4 + k));
    }
    out.push('-');
    for k in 0..f {
        out.push(bit(4 + f + k));
    }
    out
}

/// Decode + canonicalize a raw encoding the way the format defines it:
/// NaN wins; Inf&Eps together = Any; specials ignore payload; -0 -> +0.
fn decode(p: u32, f: usize) -> Val {
    let fl = |k: u32| p >> (2 * f as u32 + 3 - k) & 1 == 1;
    let (nan, inf, eps, sign) = (fl(0), fl(1), fl(2), fl(3));
    let q = (p & ((1 << (2 * f)) - 1)) as i64;
    if nan {
        Nan
    } else if inf && eps {
        Any(sign)
    } else if inf {
        Inf(sign)
    } else if eps {
        Eps(sign)
    } else if q == 0 {
        Num(0)
    } else {
        Num(if sign { -q } else { q })
    }
}

/// Canonical encoding of a semantic value as a "ffff-int-frac" bit string —
/// exactly what `nf::to_bit_string` prints for a canonical library value.
fn canon_bits(v: Val, f: usize) -> String {
    let (fl, q): ([bool; 4], i64) = match v {
        Nan => ([true, false, false, false], 0),
        Inf(s) => ([false, true, false, s], 0),
        Any(s) => ([false, true, true, s], 0),
        Eps(s) => ([false, false, true, s], 0),
        Num(q) => ([false, false, false, q < 0], q.abs()),
    };
    let mut out = String::new();
    for b in fl {
        out.push(if b { '1' } else { '0' });
    }
    out.push('-');
    for k in (0..2 * f).rev() {
        out.push(if q >> k & 1 == 1 { '1' } else { '0' });
        if k == f {
            out.push('-');
        }
    }
    out
}

fn neg(v: Val) -> Val {
    match v {
        Nan => Nan,
        Inf(s) => Inf(!s),
        Any(s) => Any(!s),
        Eps(s) => Eps(!s),
        Num(q) => Num(-q),
    }
}

fn sign_of(v: Val) -> bool {
    match v {
        Inf(s) | Any(s) | Eps(s) => s,
        Num(q) => q < 0,
        Nan => false,
    }
}

/// Round `num/den` (a value in units) to integer units, ties away from 0,
/// then apply the range: 0 with non-zero input -> Eps, above max -> Inf.
fn round_units(num: i64, den: i64, neg_sign: bool, max: i64) -> Val {
    debug_assert!(den != 0);
    let (an, ad) = (num.abs(), den.abs());
    let r = (2 * an + ad) / (2 * ad); // ties away
    if r == 0 {
        if an == 0 {
            Num(0)
        } else {
            Eps(neg_sign)
        }
    } else if r > max {
        Inf(neg_sign)
    } else {
        Num(if neg_sign { -r } else { r })
    }
}

/// Addition matrix (README "Addition").
fn o_add(a: Val, b: Val, max: i64) -> Val {
    match (a, b) {
        (Nan, _) | (_, Nan) => Nan,
        (Inf(s1), Inf(s2)) => {
            if s1 == s2 {
                Inf(s1)
            } else {
                Nan
            }
        }
        (Inf(s1), Any(s2)) | (Any(s2), Inf(s1)) => {
            if s1 == s2 {
                Inf(s1)
            } else {
                Nan
            }
        }
        (Inf(s), _) | (_, Inf(s)) => Inf(s),
        (Any(s1), Any(s2)) => {
            if s1 == s2 {
                Any(s1)
            } else {
                Nan
            }
        }
        (Any(s1), Eps(s2)) | (Eps(s2), Any(s1)) => {
            if s1 == s2 {
                Any(s1)
            } else {
                Nan
            }
        }
        (Any(s), Num(q)) | (Num(q), Any(s)) => {
            // +A absorbs anything >= 0; any opposite-signed value -> NaN
            if q == 0 || (q > 0) != s {
                Any(s)
            } else {
                Nan
            }
        }
        (Eps(s1), Eps(s2)) => {
            if s1 == s2 {
                Eps(s1)
            } else {
                Nan
            }
        }
        (Eps(s), Num(0)) | (Num(0), Eps(s)) => Eps(s),
        (Eps(_), Num(q)) | (Num(q), Eps(_)) => Num(q), // eps absorbed by normals
        (Num(q1), Num(q2)) => {
            let s = q1 + q2;
            if s.abs() > max {
                Inf(s < 0)
            } else {
                Num(s)
            }
        }
    }
}

fn o_sub(a: Val, b: Val, max: i64) -> Val {
    o_add(a, neg(b), max)
}

/// Multiplication matrix (README "Multiplication"); sign = XOR of signs.
fn o_mul(a: Val, b: Val, u: i64, max: i64) -> Val {
    if matches!(a, Nan) || matches!(b, Nan) {
        return Nan;
    }
    let sx = sign_of(a) ^ sign_of(b);
    match (a, b) {
        (Nan, _) | (_, Nan) => Nan,
        (Inf(_), Num(0)) | (Num(0), Inf(_)) => Nan,
        (Any(_), Num(0)) | (Num(0), Any(_)) => Nan,
        (Inf(_), Inf(_)) => Inf(sx),
        (Inf(_), Any(_)) | (Any(_), Inf(_)) => Any(sx),
        (Inf(_), Eps(_)) | (Eps(_), Inf(_)) => Any(sx), // the signature rule
        (Inf(_), Num(_)) | (Num(_), Inf(_)) => Inf(sx),
        (Any(_), Any(_)) | (Any(_), Eps(_)) | (Eps(_), Any(_)) => Any(sx),
        (Any(_), Num(_)) | (Num(_), Any(_)) => Any(sx),
        (Eps(_), Eps(_)) => Eps(sx),
        (Eps(_), Num(0)) | (Num(0), Eps(_)) => Num(0), // 0 * eps = exact 0
        (Eps(_), Num(_)) | (Num(_), Eps(_)) => Eps(sx),
        (Num(q1), Num(q2)) => round_units(q1 * q2, u, sx, max),
    }
}

/// Division matrix (README "Division"); sign = XOR of signs.
fn o_div(a: Val, b: Val, u: i64, max: i64) -> Val {
    if matches!(a, Nan) || matches!(b, Nan) {
        return Nan;
    }
    if matches!(b, Num(0)) {
        return Nan; // division by exact (unsigned) zero, including 0/0
    }
    let sx = sign_of(a) ^ sign_of(b);
    match (a, b) {
        (Nan, _) | (_, Nan) => Nan,
        (Num(0), _) => Num(0),
        (Inf(_), Inf(_)) => Any(sx),
        (Inf(_), Any(_)) => Any(sx),
        (Inf(_), Eps(_)) => Inf(sx),
        (Inf(_), Num(_)) => Inf(sx),
        (Any(_), _) => Any(sx),
        (Eps(_), Inf(_)) => Eps(sx),
        (Eps(_), Any(_)) => Any(sx),
        (Eps(_), Eps(_)) => Any(sx),
        (Eps(_), Num(_)) => Eps(sx),
        (Num(_), Inf(_)) => Eps(sx),
        (Num(_), Any(_)) => Any(sx),
        (Num(_), Eps(_)) => Inf(sx),
        (Num(q1), Num(q2)) => round_units(u * q1, q2, sx, max),
    }
}

/// Exact `(qa/u)^(qb/u)` for a positive normal base (`qa > 0`), rounded to
/// integer units with ties away from zero; `neg_sign` flips the sign of the
/// final result (used for negative bases with odd integral exponents).
///
/// r is the largest m with a^b >= (2m-1)/2 units, decided EXACTLY: raising
/// both sides to the u-th power turns the comparison into integer
/// arithmetic that fits u128 for u <= 4.  Ties land on the boundary and
/// count as >=, i.e. away from zero.
fn pow_units(qa: i64, qb: i64, u: i64, max: i64, neg_sign: bool) -> Val {
    let (qa, uu) = (qa as u128, u as u128);
    let c = qb.unsigned_abs() as u32;
    let up = u as u32;
    let mut r: i64 = 0;
    for m in 1..=(max + 1) {
        let odd = (2 * m - 1) as u128;
        // (qa/u)^(±c/u) >= ((2m-1)/(2u))  <=>  (u-th power, both sides > 0)
        let ge = if qb > 0 {
            qa.pow(c) * (2 * uu).pow(up) >= odd.pow(up) * uu.pow(c)
        } else {
            uu.pow(c) * (2 * uu).pow(up) >= odd.pow(up) * qa.pow(c)
        };
        if ge {
            r = m;
        } else {
            break;
        }
    }
    if r == 0 {
        Eps(neg_sign)
    } else if r > max {
        Inf(neg_sign)
    } else {
        Num(if neg_sign { -r } else { r })
    }
}

/// Exponentiation matrix (README "Exponentiation").
/// Integral exponents are the multiples of u; only they are legal for
/// negative bases (sign of the result = parity of the exponent).
fn o_pow(a: Val, b: Val, u: i64, max: i64) -> Val {
    if matches!(a, Nan) || matches!(b, Nan) {
        return Nan; // NaN poisons first, even NaN^0
    }
    if b == Num(0) {
        return Num(u); // x^0 = 1 for every non-NaN x
    }
    match b {
        // --- special exponents --------------------------------------
        Eps(sb) => match a {
            Inf(false) | Any(false) | Eps(false) => Any(false),
            Num(0) => {
                if sb {
                    Nan
                } else {
                    Num(0)
                }
            }
            Num(q) if q > 0 => Num(u), // a^(±eps) rounds to 1 for a > 0
            _ => Nan,                  // negative anything ^ non-integral
        },
        Inf(sb) => match a {
            Inf(false) => {
                if sb {
                    Eps(false)
                } else {
                    Inf(false)
                }
            }
            Any(false) => Any(false),
            Eps(false) => {
                if sb {
                    Inf(false)
                } else {
                    Eps(false)
                }
            }
            Num(0) => {
                if sb {
                    Nan
                } else {
                    Num(0)
                }
            }
            Num(q) if q == u => Nan, // 1^∞ indeterminate
            Num(q) if q > u => {
                if sb {
                    Eps(false)
                } else {
                    Inf(false)
                }
            }
            Num(q) if q > 0 => {
                if sb {
                    Inf(false)
                } else {
                    Eps(false)
                }
            }
            _ => Nan,
        },
        Any(sb) => match a {
            Inf(false) => {
                if sb {
                    Any(false)
                } else {
                    Inf(false)
                }
            }
            Any(false) | Eps(false) => Any(false),
            Num(0) => {
                if sb {
                    Nan
                } else {
                    Num(0)
                }
            }
            Num(q) if q == u => Nan, // 1^Any indeterminate
            Num(q) if q > 0 => Any(false),
            _ => Nan,
        },
        // --- normal, non-zero exponents ------------------------------
        Num(qb) => {
            let bpos = qb > 0;
            let integral = qb % u == 0;
            let odd_e = integral && (qb / u) % 2 != 0;
            match a {
                Inf(sa) => {
                    if !sa {
                        if bpos {
                            Inf(false)
                        } else {
                            Eps(false)
                        }
                    } else if integral {
                        if bpos {
                            Inf(odd_e)
                        } else {
                            Eps(odd_e)
                        }
                    } else {
                        Nan
                    }
                }
                Any(sa) => {
                    if !sa {
                        Any(false)
                    } else if integral {
                        Any(odd_e)
                    } else {
                        Nan
                    }
                }
                Eps(sa) => {
                    if !sa {
                        if bpos {
                            Eps(false)
                        } else {
                            Inf(false)
                        }
                    } else if integral {
                        if bpos {
                            Eps(odd_e)
                        } else {
                            Inf(odd_e)
                        }
                    } else {
                        Nan
                    }
                }
                Num(0) => {
                    if bpos {
                        Num(0)
                    } else {
                        Nan
                    }
                }
                Num(qa) if qa > 0 => pow_units(qa, qb, u, max, false),
                Num(qa) => {
                    // negative normal base: exponent must be integral
                    if integral {
                        pow_units(-qa, qb, u, max, odd_e)
                    } else {
                        Nan
                    }
                }
                Nan => unreachable!(),
            }
        }
        Nan => unreachable!(),
    }
}

type Oracle = fn(Val, Val, i64, i64) -> Val;

fn o_mul2(a: Val, b: Val, u: i64, max: i64) -> Val {
    o_mul(a, b, u, max)
}
fn o_div2(a: Val, b: Val, u: i64, max: i64) -> Val {
    o_div(a, b, u, max)
}
fn o_add2(a: Val, b: Val, _u: i64, max: i64) -> Val {
    o_add(a, b, max)
}
fn o_sub2(a: Val, b: Val, _u: i64, max: i64) -> Val {
    o_sub(a, b, max)
}

/// Feed all 2^(2F+4) x 2^(2F+4) raw pairs through `lib` at precision `n`,
/// requiring bit-exact agreement with `oracle` on the full output encoding.
fn run_op(n: u32, name: &str, lib: fn(&[Limb], &[Limb]) -> Vec<Limb>, oracle: Oracle) {
    with_n(n, || {
        let f = nfm::frac_bits();
        let u = 1i64 << f; // units per 1
        let max = (1i64 << (2 * f)) - 1; // largest mantissa, in units
        let total = 1u32 << (2 * f + 4);
        let mut cases = 0usize;
        for pa in 0..total {
            for pb in 0..total {
                let a = nfm::from_bit_string(&bits_str(pa, f)).unwrap();
                let b = nfm::from_bit_string(&bits_str(pb, f)).unwrap();
                let got = nfm::to_bit_string(&lib(&a, &b));
                let want = canon_bits(oracle(decode(pa, f), decode(pb, f), u, max), f);
                assert_eq!(
                    got,
                    want,
                    "{}F {}: a={} ({:?}) {} b={} ({:?})",
                    n,
                    name,
                    bits_str(pa, f),
                    decode(pa, f),
                    name,
                    bits_str(pb, f),
                    decode(pb, f),
                );
                cases += 1;
            }
        }
        assert_eq!(cases, (total as usize) * (total as usize));
    });
}

fn run_gated(n: u32, var: &str, knob: &str, name: &str, lib: fn(&[Limb], &[Limb]) -> Vec<Limb>, oracle: Oracle) {
    if !enabled(var) {
        eprintln!("all{}f: skipped (run with `make test {}=1`)", n, knob);
        return;
    }
    run_op(n, name, lib, oracle);
}

#[test]
fn all0f_add() {
    run_gated(0, "NF_ALL0F", "ALL0F", "add", nfm::add, o_add2);
}

#[test]
fn all0f_sub() {
    run_gated(0, "NF_ALL0F", "ALL0F", "sub", nfm::sub, o_sub2);
}

#[test]
fn all0f_mul() {
    run_gated(0, "NF_ALL0F", "ALL0F", "mul", nfm::mul, o_mul2);
}

#[test]
fn all0f_div() {
    run_gated(0, "NF_ALL0F", "ALL0F", "div", nfm::div, o_div2);
}

#[test]
fn all0f_pow() {
    run_gated(0, "NF_ALL0F", "ALL0F", "pow", real::pow, o_pow);
}

#[test]
fn all1f_add() {
    run_gated(1, "NF_ALL1F", "ALL1F", "add", nfm::add, o_add2);
}

#[test]
fn all1f_sub() {
    run_gated(1, "NF_ALL1F", "ALL1F", "sub", nfm::sub, o_sub2);
}

#[test]
fn all1f_mul() {
    run_gated(1, "NF_ALL1F", "ALL1F", "mul", nfm::mul, o_mul2);
}

#[test]
fn all1f_div() {
    run_gated(1, "NF_ALL1F", "ALL1F", "div", nfm::div, o_div2);
}

#[test]
fn all1f_pow() {
    run_gated(1, "NF_ALL1F", "ALL1F", "pow", real::pow, o_pow);
}

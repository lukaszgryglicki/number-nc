//! Exhaustive 2F and 0C operation checks, bit-exact.
//!
//! Both suites literally enumerate EVERY ordered pair of raw 12-bit
//! encodings for each checked operation:
//!
//!   2F: one number is [NaN][Inf][InfSmall][Sign][iiii][ffff] = 12 bits,
//!       so 4096 x 4096 = 16 777 216 pairs per op;
//!   0C: one complex number is two 0F components (6 bits each) = 12 bits,
//!       again 16 777 216 pairs per op.
//!
//! Checked operations: add, mul, div, pow — 4 x 16 777 216 = 67 108 864
//! bit-exact cases per suite (plus a 4096-case encoding/canonicalization
//! sweep).  `sub` is omitted by agreement: it is definitionally
//! `a + (-b)` (see `nc::sub`/`nf::sub`), already covered by the `add`
//! grid together with the negation matrix.  The NaN-poisoning rows are
//! kept in the grid (they cost nothing).
//!
//! To keep the runtime sane the expected outputs are precomputed once per
//! *canonical* value pair (2F has 518 canonical values -> 268 324 pairs;
//! 0C has 14 x 14 = 196 canonical complex values -> 38 416 pairs) and the
//! 16.7M raw pairs are then swept against the library with cached expected
//! strings, sharded across threads.  This is sound because the raw->
//! canonical mapping itself is verified exhaustively (the `*_encodings`
//! tests check all 4096 raw encodings against an independent decoder) and
//! every op only sees canonical parsed values.
//!
//! Oracles are independent of the library:
//!   * scalar add/mul/div: small-integer rationals in units of 2^-F,
//!     round-to-nearest ties-away, overflow -> Inf, nonzero-to-0 -> Eps
//!     (the same oracle family that is validated exhaustively at 0F/1F);
//!   * scalar pow: exact integer comparisons `a^b >= (2m-1)/(2u)` on a
//!     tiny standalone bigint (values up to ~2^2120), binary-searched —
//!     no floating point anywhere, exact ties decided away from zero;
//!   * complex ops: the README/documented semantics — exact once-rounded
//!     cross sums for all-normal operands, and the documented structural
//!     composition (term-wise NF special algebra, Smith's division,
//!     `exp(w ln z)` with correctly-rounded 0F transcendental tables) for
//!     special operands.
//!
//! Heavy and deliberately opt-in:
//!   `make test ALL2F=1` (env `NF_ALL2F`) runs the 2F suite,
//!   `make test ALL0C=1` (env `NF_ALL0C`) runs the 0C suite.
//! Without the variables the tests are no-ops.

use nf::nf::{self as nfm, with_n};
use nf::{nc, real, Limb};
use std::cmp::Ordering;
use std::time::Instant;

fn enabled(var: &str) -> bool {
    std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false)
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|v| v.get()).unwrap_or(4).min(16)
}

/// Build `(0..total).map(f)` in parallel, preserving order.
fn par_build<T: Send>(total: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let nth = threads();
    let chunk = total.div_ceil(nth);
    let mut out: Vec<T> = Vec::with_capacity(total);
    std::thread::scope(|s| {
        let mut hs = Vec::new();
        for t in 0..nth {
            let (lo, hi) = (t * chunk, ((t + 1) * chunk).min(total));
            let f = &f;
            hs.push(s.spawn(move || (lo..hi).map(f).collect::<Vec<T>>()));
        }
        for h in hs {
            out.extend(h.join().unwrap());
        }
    });
    out
}

/// Run `row(pa)` for every `pa` in `0..rows` across threads; each call
/// returns the number of cases it checked, the total is returned.
fn par_sweep(rows: usize, row: impl Fn(usize) -> usize + Sync) -> usize {
    let nth = threads();
    let chunk = rows.div_ceil(nth);
    std::thread::scope(|s| {
        let mut hs = Vec::new();
        for t in 0..nth {
            let (lo, hi) = (t * chunk, ((t + 1) * chunk).min(rows));
            let row = &row;
            hs.push(s.spawn(move || (lo..hi).map(row).sum::<usize>()));
        }
        hs.into_iter().map(|h| h.join().unwrap()).sum()
    })
}

// ---------------------------------------------------------------------------
// semantic values and encodings (the same design validated by tests/allf.rs)
// ---------------------------------------------------------------------------

/// Semantic value of an NF number.  `Num(q)` holds units of `2^-F`
/// (halves at 0F, sixteenths at 2F); zero is unsigned (q = 0).
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

/// Every canonical value at a given precision: 7 specials, zero, and
/// `±1..=±max` units (518 values at 2F, 14 at 0F).
fn canon_vals(max: i64) -> Vec<Val> {
    let mut v = vec![Nan, Inf(false), Inf(true), Any(false), Any(true), Eps(false), Eps(true), Num(0)];
    for q in 1..=max {
        v.push(Num(q));
        v.push(Num(-q));
    }
    v
}

fn pos_of(canon: &[Val], v: Val) -> usize {
    canon.iter().position(|&c| c == v).unwrap()
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

// ---------------------------------------------------------------------------
// scalar oracles (README operation matrices; validated exhaustively at 0F/1F)
// ---------------------------------------------------------------------------

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
        (Inf(_), Eps(_)) | (Eps(_), Inf(_)) => Any(sx),
        (Inf(_), Num(_)) | (Num(_), Inf(_)) => Inf(sx),
        (Any(_), Any(_)) | (Any(_), Eps(_)) | (Eps(_), Any(_)) => Any(sx),
        (Any(_), Num(_)) | (Num(_), Any(_)) => Any(sx),
        (Eps(_), Eps(_)) => Eps(sx),
        (Eps(_), Num(0)) | (Num(0), Eps(_)) => Num(0),
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
        return Nan;
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

// ---------------------------------------------------------------------------
// tiny standalone bigint (u64 limbs, little-endian) for the exact pow oracle
// ---------------------------------------------------------------------------

type Big = Vec<u64>;

fn b_norm(mut v: Big) -> Big {
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

fn b_from(x: u64) -> Big {
    b_norm(vec![x])
}

fn b_mul(a: &[u64], b: &[u64]) -> Big {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let mut r = vec![0u64; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u128;
        for (j, &y) in b.iter().enumerate() {
            let t = r[i + j] as u128 + x as u128 * y as u128 + carry;
            r[i + j] = t as u64;
            carry = t >> 64;
        }
        let mut k = i + b.len();
        while carry > 0 {
            let t = r[k] as u128 + carry;
            r[k] = t as u64;
            carry = t >> 64;
            k += 1;
        }
    }
    b_norm(r)
}

fn b_cmp(a: &[u64], b: &[u64]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    Ordering::Equal
}

fn b_pow(base: u64, e: u32) -> Big {
    let mut r = b_from(1);
    let mut b = b_from(base);
    let mut e = e;
    loop {
        if e & 1 == 1 {
            r = b_mul(&r, &b);
        }
        e >>= 1;
        if e == 0 {
            break;
        }
        b = b_mul(&b, &b);
    }
    r
}

/// Exact `(qa/u)^(qb/u)` for a positive normal base (`qa > 0`, `qb != 0`),
/// rounded to integer units with ties away from zero; `neg_sign` flips the
/// sign of the final result (negative bases with odd integral exponents).
///
/// The result is the largest m with `a^b >= (2m-1)/(2u)`, decided EXACTLY:
/// raising both sides to the u-th power turns the comparison into integer
/// arithmetic —
///   qb > 0:  qa^|qb| * (2u)^u  >=  (2m-1)^u * u^|qb|
///   qb < 0:  u^|qb| * (2u)^u   >=  (2m-1)^u * qa^|qb|
/// (integers up to ~2^2120 at 2F), binary-searched over m.  Ties land on
/// the boundary and count as >=, i.e. away from zero.
fn pow_units_big(qa: i64, qb: i64, u: i64, max: i64, neg_sign: bool) -> Val {
    debug_assert!(qa > 0 && qb != 0);
    let c = qb.unsigned_abs() as u32;
    let (up, uu) = (u as u32, u as u64);
    let a_pow = b_pow(qa as u64, c);
    let u_pow = b_pow(uu, c);
    let scale = b_pow(2 * uu, up); // (2u)^u
    let (lhs, rbase) = if qb > 0 { (b_mul(&a_pow, &scale), u_pow) } else { (b_mul(&u_pow, &scale), a_pow) };
    let p = |m: i64| b_cmp(&lhs, &b_mul(&b_pow((2 * m - 1) as u64, up), &rbase)) != Ordering::Less;
    if !p(1) {
        return Eps(neg_sign);
    }
    if p(max + 1) {
        return Inf(neg_sign);
    }
    // p(lo) true, p(hi) false: find the largest true m
    let (mut lo, mut hi) = (1i64, max + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if p(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Num(if neg_sign { -lo } else { lo })
}

/// The u128 variant used by the validated 0F/1F suites (u <= 4 only);
/// kept here to cross-check the bigint against it.
fn pow_units_small(qa: i64, qb: i64, u: i64, max: i64, neg_sign: bool) -> Val {
    let (qa, uu) = (qa as u128, u as u128);
    let c = qb.unsigned_abs() as u32;
    let up = u as u32;
    let mut r: i64 = 0;
    for m in 1..=(max + 1) {
        let odd = (2 * m - 1) as u128;
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

/// Exponentiation matrix (README "Exponentiation"), exact pow via bigint.
fn o_pow(a: Val, b: Val, u: i64, max: i64) -> Val {
    if matches!(a, Nan) || matches!(b, Nan) {
        return Nan;
    }
    if b == Num(0) {
        return Num(u); // x^0 = 1 for every non-NaN x
    }
    match b {
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
            _ => Nan,
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
                Num(qa) if qa > 0 => pow_units_big(qa, qb, u, max, false),
                Num(qa) => {
                    if integral {
                        pow_units_big(-qa, qb, u, max, odd_e)
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

#[derive(Clone, Copy)]
enum Op {
    Add,
    Mul,
    Div,
    Pow,
}

fn o_apply(op: Op, a: Val, b: Val, u: i64, max: i64) -> Val {
    match op {
        Op::Add => o_add(a, b, max),
        Op::Mul => o_mul(a, b, u, max),
        Op::Div => o_div(a, b, u, max),
        Op::Pow => o_pow(a, b, u, max),
    }
}

// ---------------------------------------------------------------------------
// 0C complex oracle: mirrors the documented nc semantics on 0F components
// ---------------------------------------------------------------------------

/// A complex value as (re, im) semantic 0F values (units of 1/2, max 3).
type CVal = (Val, Val);

const U0: i64 = 2; // units per 1 at 0F
const M0: i64 = 3; // largest mantissa in units at 0F
const CNAN: CVal = (Nan, Nan);
const CZERO: CVal = (Num(0), Num(0));
const CONE: CVal = (Num(U0), Num(0));

fn is_cnan(z: CVal) -> bool {
    z.0 == Nan || z.1 == Nan
}

fn is_czero(z: CVal) -> bool {
    z == CZERO
}

fn all_norm(z: CVal) -> bool {
    matches!(z.0, Num(_)) && matches!(z.1, Num(_))
}

fn num_of(v: Val) -> i64 {
    match v {
        Num(q) => q,
        _ => unreachable!(),
    }
}

fn o_abs(v: Val) -> Val {
    match v {
        Nan => Nan,
        Inf(_) => Inf(false),
        Any(_) => Any(false),
        Eps(_) => Eps(false),
        Num(q) => Num(q.abs()),
    }
}

/// A product term with an exact-zero factor vanishes structurally
/// (mirrors the `term` closure in `nc::mul` / `nc::div`).
fn term1(x: Val, y: Val) -> Val {
    if x == Num(0) || y == Num(0) {
        Num(0)
    } else {
        o_mul(x, y, U0, M0)
    }
}

/// Componentwise complex addition (`nc::add`).
fn o_cadd(a: CVal, b: CVal) -> CVal {
    (o_add(a.0, b.0, M0), o_add(a.1, b.1, M0))
}

/// Complex multiplication (`nc::mul`): exact once-rounded cross sums for
/// all-normal operands, structural composition otherwise.
fn o_cmul(a: CVal, b: CVal) -> CVal {
    if all_norm(a) && all_norm(b) {
        let (ar, ai, br, bi) = (num_of(a.0), num_of(a.1), num_of(b.0), num_of(b.1));
        let qre = ar * br - ai * bi; // units^2
        let qim = ar * bi + ai * br;
        return (round_units(qre, U0, qre < 0, M0), round_units(qim, U0, qim < 0, M0));
    }
    if is_cnan(a) || is_cnan(b) {
        return CNAN;
    }
    let (az, bz) = (is_czero(a), is_czero(b));
    if az || bz {
        let o = if az { b } else { a };
        let unbounded = |v: Val| matches!(v, Inf(_) | Any(_));
        return if unbounded(o.0) || unbounded(o.1) { CNAN } else { CZERO };
    }
    let rr = o_sub(term1(a.0, b.0), term1(a.1, b.1), M0);
    let ii = o_add(term1(a.0, b.1), term1(a.1, b.0), M0);
    (rr, ii)
}

/// `nf::cmp` restricted to the non-negative outputs of `abs` (README
/// comparison matrix): Any is ordered only against zero, and same-kind
/// Inf/Inf and Eps/Eps pairs are unordered (two different unknowns).
fn o_cmp_abs(a: Val, b: Val) -> Option<Ordering> {
    match (a, b) {
        (Nan, _) | (_, Nan) => None,
        (Any(_), Num(0)) => Some(Ordering::Greater),
        (Num(0), Any(_)) => Some(Ordering::Less),
        (Any(_), _) | (_, Any(_)) => None,
        (Inf(_), Inf(_)) => None,
        (Inf(_), _) => Some(Ordering::Greater),
        (_, Inf(_)) => Some(Ordering::Less),
        (Eps(_), Eps(_)) => None,
        (Eps(_), Num(0)) => Some(Ordering::Greater),
        (Num(0), Eps(_)) => Some(Ordering::Less),
        (Eps(_), Num(_)) => Some(Ordering::Less),
        (Num(_), Eps(_)) => Some(Ordering::Greater),
        (Num(x), Num(y)) => Some(x.cmp(&y)),
    }
}

/// Complex division (`nc::div`): exact big-rational components for
/// all-normal operands, Smith's algorithm on NF components otherwise.
fn o_cdiv(a: CVal, b: CVal) -> CVal {
    if all_norm(a) && all_norm(b) {
        if is_czero(b) {
            return CNAN;
        }
        let (ar, ai, br, bi) = (num_of(a.0), num_of(a.1), num_of(b.0), num_of(b.1));
        let den = br * br + bi * bi; // units^2, > 0
        let nre = ar * br + ai * bi;
        let nim = ai * br - ar * bi;
        let q = |n: i64| if n == 0 { Num(0) } else { round_units(U0 * n, den, n < 0, M0) };
        return (q(nre), q(nim));
    }
    if is_cnan(a) || is_cnan(b) {
        return CNAN;
    }
    let (ar, ai, br, bi) = (a.0, a.1, b.0, b.1);
    let dv = |x: Val, y: Val| o_div(x, y, U0, M0);
    if o_cmp_abs(o_abs(br), o_abs(bi)) != Some(Ordering::Less) {
        let r = dv(bi, br);
        let den = o_add(br, term1(bi, r), M0);
        let rr = dv(o_add(ar, term1(ai, r), M0), den);
        let ii = dv(o_sub(ai, term1(ar, r), M0), den);
        (rr, ii)
    } else {
        let r = dv(br, bi);
        let den = o_add(term1(br, r), bi, M0);
        let rr = dv(o_add(term1(ar, r), ai, M0), den);
        let ii = dv(o_sub(term1(ai, r), ar, M0), den);
        (rr, ii)
    }
}

// --- correctly-rounded 0F transcendental tables (units of 1/2, max 3) ------
//
// Normal-argument entries are round-to-nearest ties-away values of the true
// function, computed independently (the argument grid is only ±{0.5,1,1.5}):
// e.g. exp(0.5)=1.64872 -> 3.297 units -> 3; cos(1.5)=0.07074 -> 0.141 units
// -> +Eps; atan(3)=1.24905 -> 2.498 units -> 2.  Special rows follow the
// README unary tables.  No value lies near a tie boundary (the library
// computes with 128 guard bits, and exact ties are impossible for these
// transcendentals at rational arguments).

fn o_exp1(v: Val) -> Val {
    match v {
        Nan => Nan,
        Any(_) => Any(false),
        Inf(s) => {
            if s {
                Eps(false)
            } else {
                Inf(false)
            }
        }
        Eps(_) | Num(0) => Num(U0), // exp(±eps) = exp(0) = 1
        Num(q) => match q {
            -3 => Eps(false),    // e^-1.5 = 0.2231 -> 0.446 units
            -2 | -1 => Num(1),   // e^-1 = 0.7358u; e^-0.5 = 1.213u
            1 => Num(3),         // e^0.5 = 3.297 units
            _ => Inf(false),     // e^1 = 5.44u, e^1.5 = 8.96u: > 3
        },
    }
}

fn o_ln1(v: Val) -> Val {
    match v {
        Nan | Any(_) => Nan,
        Inf(true) | Eps(true) => Nan, // negative arguments
        Num(q) if q < 0 => Nan,
        Num(0) | Eps(false) => Inf(true),
        Inf(false) => Inf(false),
        Num(1) => Num(-1), // ln 0.5 = -1.386 units
        Num(2) => Num(0),  // ln 1 = exact 0
        Num(_) => Num(1),  // ln 1.5 = 0.811 units
    }
}

fn o_sin1(v: Val) -> Val {
    match v {
        Nan | Inf(_) | Any(_) => Nan,
        Eps(s) => Eps(s),
        Num(0) => Num(0),
        Num(q) => {
            // sin 0.5 = 0.959u -> 1; sin 1 = 1.683u -> 2; sin 1.5 = 1.995u -> 2
            let m = if q.abs() == 1 { 1 } else { 2 };
            Num(if q < 0 { -m } else { m })
        }
    }
}

fn o_cos1(v: Val) -> Val {
    match v {
        Nan | Inf(_) | Any(_) => Nan,
        Eps(_) | Num(0) => Num(U0),
        Num(q) => match q.abs() {
            1 => Num(2),      // cos 0.5 = 1.755 units
            2 => Num(1),      // cos 1 = 1.081 units
            _ => Eps(false),  // cos 1.5 = 0.141 units -> +Eps
        },
    }
}

/// π in 0F units is 6.28 -> 6 > 3 -> ±Inf; π/2 is 3.14 -> 3 units (= 1.5).
fn pi1(negv: bool) -> Val {
    Inf(negv)
}

fn half_pi1(negv: bool) -> Val {
    Num(if negv { -3 } else { 3 })
}

/// `real::atan2` at 0F: the full special-value branch ladder plus the
/// both-normal quadrant logic with a single final rounding.
fn o_atan2_1(y: Val, x: Val) -> Val {
    if y == Nan || x == Nan {
        return Nan;
    }
    if matches!(y, Any(_)) || matches!(x, Any(_)) {
        if y == Num(0) {
            return if sign_of(x) { pi1(false) } else { Num(0) };
        }
        return Any(sign_of(y));
    }
    let (y0, x0) = (y == Num(0), x == Num(0));
    if y0 && x0 {
        return Nan;
    }
    if matches!(y, Inf(_)) {
        if matches!(x, Inf(_)) {
            return Nan;
        }
        return half_pi1(sign_of(y));
    }
    if matches!(x, Inf(_)) {
        if sign_of(x) {
            return pi1(sign_of(y) && !y0);
        }
        return if y0 { Num(0) } else { Eps(sign_of(y)) };
    }
    if x0 {
        return half_pi1(sign_of(y));
    }
    if matches!(x, Eps(_)) && matches!(y, Eps(_)) {
        return Nan;
    }
    if y0 {
        return if sign_of(x) { pi1(false) } else { Num(0) };
    }
    if matches!(y, Eps(_)) {
        return if sign_of(x) { pi1(sign_of(y)) } else { Eps(sign_of(y)) };
    }
    if matches!(x, Eps(_)) {
        return half_pi1(sign_of(y));
    }
    // both normal non-zero
    let (qy, qx) = (num_of(y), num_of(x));
    if qx > 0 {
        // atan(|y/x|): >= 1.5708 units (-> 2) iff |y| >= |x|, else
        // atan(r <= 2/3) <= 1.176 units -> 1
        let t = if qy.abs() >= qx { 2 } else { 1 };
        Num(if qy < 0 { -t } else { t })
    } else {
        // x < 0: |base ± π| in units is 3.79..5.64 -> rounds above 3 -> ±Inf
        Inf(qy < 0)
    }
}

/// Complex modulus (`nc::abs`) as an NF value.
fn o_cabs(z: CVal) -> Val {
    let (r, i) = z;
    if r == Nan || i == Nan {
        return Nan;
    }
    if matches!(r, Inf(_)) || matches!(i, Inf(_)) {
        return Inf(false);
    }
    if matches!(r, Any(_)) || matches!(i, Any(_)) {
        return Any(false);
    }
    let (re_e, im_e) = (matches!(r, Eps(_)), matches!(i, Eps(_)));
    let (re_z, im_z) = (r == Num(0), i == Num(0));
    if (re_e || re_z) && (im_e || im_z) {
        return if re_e || im_e { Eps(false) } else { Num(0) };
    }
    if re_e || re_z {
        return o_abs(i);
    }
    if im_e || im_z {
        return o_abs(r);
    }
    // both normal non-zero: round(sqrt(qr^2 + qi^2)) = (isqrt(4s) + 1) >> 1
    let (qr, qi) = (num_of(r), num_of(i));
    let s = qr * qr + qi * qi;
    let mut root = 0i64;
    while (root + 1) * (root + 1) <= 4 * s {
        root += 1;
    }
    let h = (root + 1) >> 1;
    if h > M0 {
        Inf(false)
    } else {
        Num(h)
    }
}

/// `nc::exp`: e^re * (cos im + i sin im) with structural zeros.
fn o_cexp(v: CVal) -> CVal {
    let er = o_exp1(v.0);
    let (c, s) = (o_cos1(v.1), o_sin1(v.1));
    let rr = if c == Num(0) { Num(0) } else { o_mul(er, c, U0, M0) };
    let ii = if s == Num(0) { Num(0) } else { o_mul(er, s, U0, M0) };
    (rr, ii)
}

/// `nc::ln`: (ln |z|, atan2(im, re)).
fn o_cln(z: CVal) -> CVal {
    (o_ln1(o_cabs(z)), o_atan2_1(z.1, z.0))
}

/// `nc::pow` branch ladder.
fn o_cpow(z: CVal, w: CVal) -> CVal {
    if is_cnan(z) || is_cnan(w) {
        return CNAN;
    }
    if is_czero(w) {
        return CONE; // z^0 = 1, including 0^0
    }
    let w_real = w.1 == Num(0);
    if w_real && z.1 == Num(0) && !sign_of(z.0) {
        return (o_pow(z.0, w.0, U0, M0), Num(0)); // real^real via real pow
    }
    if is_czero(z) {
        return CNAN; // 0^(complex w), w != 0
    }
    if w_real {
        if let Num(q) = w.0 {
            if q != 0 && q % U0 == 0 {
                // real integral exponent; at 0F only ±1 is reachable
                debug_assert!(q.abs() == U0);
                return if q > 0 { z } else { o_cdiv(CONE, z) };
            }
        }
    }
    o_cexp(o_cmul(w, o_cln(z)))
}

fn oc_apply(op: Op, a: CVal, b: CVal) -> CVal {
    match op {
        Op::Add => o_cadd(a, b),
        Op::Mul => o_cmul(a, b),
        Op::Div => o_cdiv(a, b),
        Op::Pow => o_cpow(a, b),
    }
}

// ---------------------------------------------------------------------------
// suite runners
// ---------------------------------------------------------------------------

type LibOp = fn(&[Limb], &[Limb]) -> Vec<Limb>;

/// All 4096 raw 2F encodings parse + canonicalize correctly.
fn run_2f_encodings() {
    with_n(2, || {
        let f = nfm::frac_bits();
        for p in 0..(1u32 << (2 * f + 4)) {
            let v = nfm::from_bit_string(&bits_str(p, f)).unwrap();
            assert_eq!(nfm::to_bit_string(&v), canon_bits(decode(p, f), f), "raw {}", bits_str(p, f));
        }
    });
}

/// One 2F operation over ALL 4096 x 4096 raw pairs, bit-exact.
fn run_2f(name: &str, lib: LibOp, op: Op) {
    with_n(2, || {
        let f = nfm::frac_bits(); // 4
        let u = 1i64 << f;
        let max = (1i64 << (2 * f)) - 1;
        let total = 1usize << (2 * f + 4); // 4096
        let parsed: Vec<Vec<Limb>> =
            (0..total).map(|p| nfm::from_bit_string(&bits_str(p as u32, f)).unwrap()).collect();
        let canon = canon_vals(max); // 518 canonical values
        let ncv = canon.len();
        let cidx: Vec<usize> = (0..total).map(|p| pos_of(&canon, decode(p as u32, f))).collect();
        let expected: Vec<String> =
            par_build(ncv * ncv, |k| canon_bits(o_apply(op, canon[k / ncv], canon[k % ncv], u, max), f));
        let t0 = Instant::now();
        let done = par_sweep(total, |pa| {
            let a = &parsed[pa];
            let row = cidx[pa] * ncv;
            let mut cnt = 0usize;
            for pb in 0..total {
                let got = nfm::to_bit_string(&lib(a, &parsed[pb]));
                let want = &expected[row + cidx[pb]];
                assert!(
                    &got == want,
                    "2F {name}: {} ({:?}) {name} {} ({:?}): got {got} want {want}",
                    bits_str(pa as u32, f),
                    decode(pa as u32, f),
                    bits_str(pb as u32, f),
                    decode(pb as u32, f),
                );
                cnt += 1;
            }
            cnt
        });
        assert_eq!(done, total * total);
        eprintln!("all2f {name}: {done} bit-exact cases in {:.2?}", t0.elapsed());
    });
}

/// All 4096 raw 0C encodings (re, im 0F components) parse + canonicalize.
fn run_0c_encodings() {
    with_n(0, || {
        let f = nfm::frac_bits(); // 1
        for p in 0..4096u32 {
            let z = nc::make(
                &nfm::from_bit_string(&bits_str(p >> 6, f)).unwrap(),
                &nfm::from_bit_string(&bits_str(p & 63, f)).unwrap(),
            );
            assert_eq!(nfm::to_bit_string(nc::re(&z)), canon_bits(decode(p >> 6, f), f), "raw re {p:03x}");
            assert_eq!(nfm::to_bit_string(nc::im(&z)), canon_bits(decode(p & 63, f), f), "raw im {p:03x}");
        }
    });
}

/// One 0C operation over ALL 4096 x 4096 raw complex pairs, bit-exact.
fn run_0c(name: &str, lib: LibOp, op: Op) {
    with_n(0, || {
        let f = nfm::frac_bits(); // 1
        assert_eq!(f, 1);
        let comp_total = 1usize << (2 * f + 4); // 64 raw component encodings
        let total = comp_total * comp_total; // 4096 raw complex encodings
        let comp_parsed: Vec<Vec<Limb>> =
            (0..comp_total).map(|p| nfm::from_bit_string(&bits_str(p as u32, f)).unwrap()).collect();
        let canon = canon_vals(M0); // 14 canonical component values
        let ncv = canon.len();
        let comp_cidx: Vec<usize> = (0..comp_total).map(|p| pos_of(&canon, decode(p as u32, f))).collect();
        let parsed: Vec<Vec<Limb>> =
            (0..total).map(|p| nc::make(&comp_parsed[p >> 6], &comp_parsed[p & 63])).collect();
        let cidx: Vec<usize> = (0..total).map(|p| comp_cidx[p >> 6] * ncv + comp_cidx[p & 63]).collect();
        let cc = ncv * ncv; // 196 canonical complex values
        let expected: Vec<(String, String)> = par_build(cc * cc, |k| {
            let (ia, ib) = (k / cc, k % cc);
            let r = oc_apply(op, (canon[ia / ncv], canon[ia % ncv]), (canon[ib / ncv], canon[ib % ncv]));
            (canon_bits(r.0, f), canon_bits(r.1, f))
        });
        let t0 = Instant::now();
        let done = par_sweep(total, |pa| {
            let a = &parsed[pa];
            let row = cidx[pa] * cc;
            let mut cnt = 0usize;
            for pb in 0..total {
                let r = lib(a, &parsed[pb]);
                let (gr, gi) = (nfm::to_bit_string(nc::re(&r)), nfm::to_bit_string(nc::im(&r)));
                let want = &expected[row + cidx[pb]];
                assert!(
                    gr == want.0 && gi == want.1,
                    "0C {name}: a=({:?}, {:?}) {name} b=({:?}, {:?}): got ({gr}, {gi}) want ({}, {})",
                    decode((pa >> 6) as u32, f),
                    decode((pa & 63) as u32, f),
                    decode((pb >> 6) as u32, f),
                    decode((pb & 63) as u32, f),
                    want.0,
                    want.1,
                );
                cnt += 1;
            }
            cnt
        });
        assert_eq!(done, total * total);
        eprintln!("all0c {name}: {done} bit-exact cases in {:.2?}", t0.elapsed());
    });
}

fn gated(var: &str, knob: &str, run: impl FnOnce()) {
    if !enabled(var) {
        eprintln!("skipped (run with `make test {knob}=1`)");
        return;
    }
    run();
}

// ---------------------------------------------------------------------------
// always-on sanity checks for the oracle machinery itself
// ---------------------------------------------------------------------------

#[test]
fn oracle_bigint_matches_u128_pow() {
    // the bigint pow oracle must agree with the u128 formula that the
    // exhaustive 0F/1F suites validated bit-exactly against the library
    for &(u, max) in &[(2i64, 3i64), (4, 15)] {
        for qa in 1..=max {
            for qb in -max..=max {
                if qb == 0 {
                    continue;
                }
                for ns in [false, true] {
                    assert_eq!(
                        pow_units_big(qa, qb, u, max, ns),
                        pow_units_small(qa, qb, u, max, ns),
                        "qa={qa} qb={qb} u={u} ns={ns}"
                    );
                }
            }
        }
    }
}

#[test]
fn oracle_bigint_pow_spot_checks() {
    let (u, max) = (16, 255);
    assert_eq!(pow_units_big(8, 80, u, max, false), Num(1)); // 0.5^5 = 1/32: exact tie, away
    assert_eq!(pow_units_big(4, 40, u, max, false), Num(1)); // 0.25^2.5 = 1/32: exact tie
    assert_eq!(pow_units_big(36, 24, u, max, false), Num(54)); // 2.25^1.5 = 3.375 exactly
    assert_eq!(pow_units_big(16, 255, u, max, false), Num(16)); // 1^x = 1
    assert_eq!(pow_units_big(32, 16, u, max, false), Num(32)); // 2^1 = 2
    assert_eq!(pow_units_big(32, -16, u, max, false), Num(8)); // 2^-1 = 0.5
    assert_eq!(pow_units_big(255, 255, u, max, false), Inf(false)); // 15.94^15.94
    assert_eq!(pow_units_big(1, 255, u, max, false), Eps(false)); // 0.0625^15.94
    assert_eq!(pow_units_big(2, 8, u, max, false), Num(6)); // 0.125^0.5 = 0.3536 -> 5.66u
}

// ---------------------------------------------------------------------------
// the gated exhaustive suites
// ---------------------------------------------------------------------------

#[test]
fn all2f_encodings() {
    gated("NF_ALL2F", "ALL2F", run_2f_encodings);
}

#[test]
fn all2f_add() {
    gated("NF_ALL2F", "ALL2F", || run_2f("add", nfm::add, Op::Add));
}

#[test]
fn all2f_mul() {
    gated("NF_ALL2F", "ALL2F", || run_2f("mul", nfm::mul, Op::Mul));
}

#[test]
fn all2f_div() {
    gated("NF_ALL2F", "ALL2F", || run_2f("div", nfm::div, Op::Div));
}

#[test]
fn all2f_pow() {
    gated("NF_ALL2F", "ALL2F", || run_2f("pow", real::pow, Op::Pow));
}

#[test]
fn all0c_encodings() {
    gated("NF_ALL0C", "ALL0C", run_0c_encodings);
}

#[test]
fn all0c_add() {
    gated("NF_ALL0C", "ALL0C", || run_0c("add", nc::add, Op::Add));
}

#[test]
fn all0c_mul() {
    gated("NF_ALL0C", "ALL0C", || run_0c("mul", nc::mul, Op::Mul));
}

#[test]
fn all0c_div() {
    gated("NF_ALL0C", "ALL0C", || run_0c("div", nc::div, Op::Div));
}

#[test]
fn all0c_pow() {
    gated("NF_ALL0C", "ALL0C", || run_0c("pow", nc::pow, Op::Pow));
}

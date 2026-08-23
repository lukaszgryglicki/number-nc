//! Exhaustive special-value matrices.
//!
//! Every binary operation is checked over the full 10x10 grid of flag
//! states (NaN, +Inf, -Inf, +Any, -Any, +InfSmall, -InfSmall, 0, 1, -1) and
//! every unary operation over all special states, for every test precision
//! N in {0, 1, 2, 3, 6, 7, 8} plus the huge N = 18 (524 292 logical
//! bits, ~64 KB per number).  `make test N=K` (the `NF_TEST_N` env var)
//! additionally runs everything at N = K.  All 16 raw flag nibbles are
//! decoded and canonicalized, bit-string I/O is round-tripped, and the
//! same algebra is verified end-to-end through the expression parser.
//!
//! At huge N (>= 16) the few paths that are inherently quadratic in the
//! mantissa width (Newton roots of normal values, pi-valued results,
//! shortest-form decimal printing of dense mantissas) are skipped; their
//! correctness is already covered at every N <= 8, and tests/huge.rs
//! stresses huge N further.

use nf::nf::{self as nfm, with_n};
use nf::{expr, nc, real, Limb};

/// Every test precision: the small set {0, 1, 2, 3, 6, 7, 8} plus the huge
/// N = 18 — flag algebra and sparse-mantissa arithmetic stay fast at any
/// width.
const NS_FIXED: [u32; 8] = [0, 1, 2, 3, 6, 7, 8, 18];

/// The fixed precisions, plus `NF_TEST_N` when set (`make test N=K`).
fn ns_huge() -> Vec<u32> {
    let mut v = NS_FIXED.to_vec();
    if let Some(k) = std::env::var("NF_TEST_N").ok().and_then(|s| s.parse().ok()) {
        if !v.contains(&k) {
            v.push(k);
        }
    }
    v
}

/// True for precisions where O(mantissa^2) work must be avoided.
fn is_huge(n: u32) -> bool {
    n >= 16
}

const STATES: [&str; 10] =
    ["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0", "1", "-1"];

fn mk(s: &str) -> Vec<Limb> {
    match s {
        "nan" => nfm::nan(),
        "+inf" => nfm::inf(false),
        "-inf" => nfm::inf(true),
        "+any" => nfm::any(false),
        "-any" => nfm::any(true),
        "+eps" => nfm::eps(false),
        "-eps" => nfm::eps(true),
        _ => nfm::from_str(s).unwrap(),
    }
}

/// Classify a value into a state string ("nan", "+inf", ..., or its digits).
fn st(v: &[Limb]) -> String {
    if nfm::is_nan(v) {
        return "nan".into();
    }
    if nfm::is_any(v) {
        return if nfm::is_neg(v) { "-any".into() } else { "+any".into() };
    }
    if nfm::is_inf(v) {
        return if nfm::is_neg(v) { "-inf".into() } else { "+inf".into() };
    }
    if nfm::is_eps(v) {
        return if nfm::is_neg(v) { "-eps".into() } else { "+eps".into() };
    }
    nfm::to_string_digits(v, None)
}

/// A literal is usable at the current N when it survives a parse/print trip.
fn representable(lit: &str) -> bool {
    st(&mk(lit)) == lit
}

fn check_grid(
    name: &str,
    op: impl Fn(&[Limb], &[Limb]) -> Vec<Limb>,
    rows: &[&str],
    cols: &[&str],
    grid: &[&[&str]],
) {
    for &n in &ns_huge() {
        with_n(n, || {
            for (i, &sa) in rows.iter().enumerate() {
                for (j, &sb) in cols.iter().enumerate() {
                    let want = grid[i][j];
                    if want == "."
                        || !representable(sa)
                        || !representable(sb)
                        || !representable(want)
                    {
                        continue;
                    }
                    let got = st(&op(&mk(sa), &mk(sb)));
                    assert_eq!(
                        got, want,
                        "{}({}, {}) at N={}: got {}, want {}",
                        name, sa, sb, n, got, want
                    );
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// addition / subtraction
// ---------------------------------------------------------------------------

#[test]
fn add_matrix() {
    let g: &[&[&str]] = &[
        //         nan    +inf   -inf   +any   -any   +eps   -eps   0      1      -1
        /*nan */ &["nan"; 10],
        /*+inf*/ &["nan", "+inf", "nan", "+inf", "nan", "+inf", "+inf", "+inf", "+inf", "+inf"],
        /*-inf*/ &["nan", "nan", "-inf", "nan", "-inf", "-inf", "-inf", "-inf", "-inf", "-inf"],
        /*+any*/ &["nan", "+inf", "nan", "+any", "nan", "+any", "nan", "+any", "+any", "nan"],
        /*-any*/ &["nan", "nan", "-inf", "nan", "-any", "nan", "-any", "-any", "nan", "-any"],
        /*+eps*/ &["nan", "+inf", "-inf", "+any", "nan", "+eps", "nan", "+eps", "1", "-1"],
        /*-eps*/ &["nan", "+inf", "-inf", "nan", "-any", "nan", "-eps", "-eps", "1", "-1"],
        /*  0 */ &["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0", "1", "-1"],
        /*  1 */ &["nan", "+inf", "-inf", "+any", "nan", "1", "1", "1", "2", "0"],
        /* -1 */ &["nan", "+inf", "-inf", "nan", "-any", "-1", "-1", "-1", "0", "-2"],
    ];
    check_grid("add", nfm::add, &STATES, &STATES, g);
}

#[test]
fn sub_is_add_neg() {
    // sub(a,b) must agree with add(a, -b) on every pair of states, every N
    for &n in &ns_huge() {
        with_n(n, || {
            for &sa in &STATES {
                for &sb in &STATES {
                    let (a, b) = (mk(sa), mk(sb));
                    let via_add = st(&nfm::add(&a, &nfm::neg(&b)));
                    let direct = st(&nfm::sub(&a, &b));
                    assert_eq!(direct, via_add, "sub({}, {}) at N={}", sa, sb, n);
                }
            }
        });
    }
}

#[test]
fn sub_matrix_spots() {
    with_n(3, || {
        assert_eq!(st(&nfm::sub(&mk("+inf"), &mk("+inf"))), "nan");
        assert_eq!(st(&nfm::sub(&mk("+inf"), &mk("-inf"))), "+inf");
        assert_eq!(st(&nfm::sub(&mk("+any"), &mk("+any"))), "nan");
        assert_eq!(st(&nfm::sub(&mk("+any"), &mk("-any"))), "+any");
        assert_eq!(st(&nfm::sub(&mk("+eps"), &mk("+eps"))), "nan");
        assert_eq!(st(&nfm::sub(&mk("+eps"), &mk("-eps"))), "+eps");
        assert_eq!(st(&nfm::sub(&mk("1"), &mk("-1"))), "2");
        assert_eq!(st(&nfm::sub(&mk("1"), &mk("1"))), "0");
        assert_eq!(st(&nfm::sub(&mk("0"), &mk("+any"))), "-any");
        assert_eq!(st(&nfm::sub(&mk("0"), &mk("-eps"))), "+eps");
    });
}

// ---------------------------------------------------------------------------
// multiplication / division
// ---------------------------------------------------------------------------

#[test]
fn mul_matrix() {
    let g: &[&[&str]] = &[
        //         nan    +inf   -inf   +any   -any   +eps   -eps   0      1      -1
        /*nan */ &["nan"; 10],
        /*+inf*/ &["nan", "+inf", "-inf", "+any", "-any", "+any", "-any", "nan", "+inf", "-inf"],
        /*-inf*/ &["nan", "-inf", "+inf", "-any", "+any", "-any", "+any", "nan", "-inf", "+inf"],
        /*+any*/ &["nan", "+any", "-any", "+any", "-any", "+any", "-any", "nan", "+any", "-any"],
        /*-any*/ &["nan", "-any", "+any", "-any", "+any", "-any", "+any", "nan", "-any", "+any"],
        /*+eps*/ &["nan", "+any", "-any", "+any", "-any", "+eps", "-eps", "0", "+eps", "-eps"],
        /*-eps*/ &["nan", "-any", "+any", "-any", "+any", "-eps", "+eps", "0", "-eps", "+eps"],
        /*  0 */ &["nan", "nan", "nan", "nan", "nan", "0", "0", "0", "0", "0"],
        /*  1 */ &["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0", "1", "-1"],
        /* -1 */ &["nan", "-inf", "+inf", "-any", "+any", "-eps", "+eps", "0", "-1", "1"],
    ];
    check_grid("mul", nfm::mul, &STATES, &STATES, g);
}

#[test]
fn div_matrix() {
    let g: &[&[&str]] = &[
        //         nan    +inf   -inf   +any   -any   +eps   -eps   0      1      -1
        /*nan */ &["nan"; 10],
        /*+inf*/ &["nan", "+any", "-any", "+any", "-any", "+inf", "-inf", "nan", "+inf", "-inf"],
        /*-inf*/ &["nan", "-any", "+any", "-any", "+any", "-inf", "+inf", "nan", "-inf", "+inf"],
        /*+any*/ &["nan", "+any", "-any", "+any", "-any", "+any", "-any", "nan", "+any", "-any"],
        /*-any*/ &["nan", "-any", "+any", "-any", "+any", "-any", "+any", "nan", "-any", "+any"],
        /*+eps*/ &["nan", "+eps", "-eps", "+any", "-any", "+any", "-any", "nan", "+eps", "-eps"],
        /*-eps*/ &["nan", "-eps", "+eps", "-any", "+any", "-any", "+any", "nan", "-eps", "+eps"],
        /*  0 */ &["nan", "0", "0", "0", "0", "0", "0", "nan", "0", "0"],
        /*  1 */ &["nan", "+eps", "-eps", "+any", "-any", "+inf", "-inf", "nan", "1", "-1"],
        /* -1 */ &["nan", "-eps", "+eps", "-any", "+any", "-inf", "+inf", "nan", "-1", "1"],
    ];
    check_grid("div", nfm::div, &STATES, &STATES, g);
}

// ---------------------------------------------------------------------------
// power
// ---------------------------------------------------------------------------

#[test]
fn pow_matrix() {
    let bases: [&str; 14] = [
        "nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0", "1", "-1", "2", "-2", "0.5",
        "-0.5",
    ];
    let exps: [&str; 12] =
        ["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0", "2", "-2", "3", "0.5"];
    let g: &[&[&str]] = &[
        //          nan    +inf   -inf   +any   -any   +eps   -eps   0    2      -2      3       0.5
        /*nan  */ &["nan"; 12],
        /*+inf */ &["nan", "+inf", "+eps", "+inf", "+any", "+any", "+any", "1", "+inf", "+eps", "+inf", "+inf"],
        /*-inf */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "+inf", "+eps", "-inf", "nan"],
        /*+any */ &["nan", "+any", "+any", "+any", "+any", "+any", "+any", "1", "+any", "+any", "+any", "+any"],
        /*-any */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "+any", "+any", "-any", "nan"],
        /*+eps */ &["nan", "+eps", "+inf", "+any", "+any", "+any", "+any", "1", "+eps", "+inf", "+eps", "+eps"],
        /*-eps */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "+eps", "+inf", "-eps", "nan"],
        /*  0  */ &["nan", "0", "nan", "0", "nan", "0", "nan", "1", "0", "nan", "0", "0"],
        /*  1  */ &["nan", "nan", "nan", "nan", "nan", "1", "1", "1", "1", "1", "1", "1"],
        /* -1  */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "1", "1", "-1", "nan"],
        /*  2  */ &["nan", "+inf", "+eps", "+any", "+any", "1", "1", "1", "4", "0.25", "8", "."],
        /* -2  */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "4", "0.25", "-8", "nan"],
        /* 0.5 */ &["nan", "+eps", "+inf", "+any", "+any", "1", "1", "1", "0.25", "4", "0.125", "."],
        /*-0.5 */ &["nan", "nan", "nan", "nan", "nan", "nan", "nan", "1", "0.25", "4", "-0.125", "nan"],
    ];
    check_grid("pow", real::pow, &bases, &exps, g);
}

#[test]
fn pow_numeric_spots() {
    // 1024 needs 11 integer bits -> N>=4; at N in {2,3} it must overflow
    for &(n, want) in &[(2u32, "+inf"), (3, "+inf"), (6, "1024"), (7, "1024"), (8, "1024")] {
        with_n(n, || {
            assert_eq!(st(&real::pow(&mk("2"), &mk("10"))), want, "2^10 at N={}", n);
        });
    }
}

// ---------------------------------------------------------------------------
// roots and squares
// ---------------------------------------------------------------------------

#[test]
fn root_matrix() {
    // k = 0 -> NaN for everything
    for &n in &ns_huge() {
        with_n(n, || {
            for &s in &STATES {
                assert_eq!(st(&real::nth_root(&mk(s), 0)), "nan", "root({}, 0) at N={}", s, n);
            }
        });
    }
    // rows: states, cols: k in {1, 2, 3, 4}
    let g: &[&[&str]] = &[
        /*nan */ &["nan", "nan", "nan", "nan"],
        /*+inf*/ &["+inf", "+inf", "+inf", "+inf"],
        /*-inf*/ &["-inf", "nan", "-inf", "nan"],
        /*+any*/ &["+any", "+any", "+any", "+any"],
        /*-any*/ &["-any", "nan", "-any", "nan"],
        /*+eps*/ &["+eps", "+eps", "+eps", "+eps"],
        /*-eps*/ &["-eps", "nan", "-eps", "nan"],
        /*  0 */ &["0", "0", "0", "0"],
        /*  1 */ &["1", "1", "1", "1"],
        /* -1 */ &["-1", "nan", "-1", "nan"],
    ];
    for &n in &ns_huge() {
        with_n(n, || {
            for (i, &s) in STATES.iter().enumerate() {
                // Newton roots of normal values are O(width^2): skip at huge N
                if is_huge(n) && matches!(s, "1" | "-1") {
                    continue;
                }
                for (kj, &k) in [1u64, 2, 3, 4].iter().enumerate() {
                    let got = st(&real::nth_root(&mk(s), k));
                    assert_eq!(got, g[i][kj], "root({}, {}) at N={}", s, k, n);
                }
            }
        });
    }
}

#[test]
fn root_numeric_spots() {
    for &n in &[2u32, 3, 6, 7, 8] {
        with_n(n, || {
            assert_eq!(st(&real::nth_root(&mk("4"), 2)), "2");
            assert_eq!(st(&real::nth_root(&mk("8"), 3)), "2");
            assert_eq!(st(&real::nth_root(&mk("-8"), 3)), "-2");
            assert_eq!(st(&real::sqrt(&mk("0.25"))), "0.5");
        });
    }
}

#[test]
fn square_consistency() {
    // square(a) == mul(a, a) == pow(a, 2) on all states (pow needs "2")
    for &n in &ns_huge() {
        with_n(n, || {
            let two_ok = representable("2");
            for &s in &STATES {
                let a = mk(s);
                let m = st(&nfm::mul(&a, &a));
                if two_ok {
                    let p = st(&real::pow(&a, &mk("2")));
                    assert_eq!(m, p, "square {} at N={}", s, n);
                }
                // sqrt(square(x)) == abs-ish state for non-NaN inputs
                // (skipped for normal values at huge N: Newton is O(width^2))
                if is_huge(n) && matches!(s, "1" | "-1") {
                    continue;
                }
                let sq = nfm::mul(&a, &a);
                let back = st(&real::sqrt(&sq));
                let want = match s {
                    "nan" => "nan".to_string(),
                    "+inf" | "-inf" => "+inf".to_string(),
                    "+any" | "-any" => "+any".to_string(),
                    "+eps" | "-eps" => "+eps".to_string(),
                    "0" => "0".to_string(),
                    v => st(&nfm::abs(&mk(v))),
                };
                assert_eq!(back, want, "sqrt(square({})) at N={}", s, n);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// comparison
// ---------------------------------------------------------------------------

#[test]
fn cmp_matrix() {
    use std::cmp::Ordering::*;
    let sym = |c: Option<std::cmp::Ordering>| match c {
        None => "?",
        Some(Less) => "<",
        Some(Equal) => "=",
        Some(Greater) => ">",
    };
    let g: &[&[&str]] = &[
        //         nan  +inf -inf +any -any +eps -eps  0    1    -1
        /*nan */ &["?", "?", "?", "?", "?", "?", "?", "?", "?", "?"],
        /*+inf*/ &["?", "?", ">", "?", ">", ">", ">", ">", ">", ">"],
        /*-inf*/ &["?", "<", "?", "<", "?", "<", "<", "<", "<", "<"],
        /*+any*/ &["?", "?", ">", "?", ">", "?", ">", ">", "?", ">"],
        /*-any*/ &["?", "<", "?", "<", "?", "<", "?", "<", "<", "?"],
        /*+eps*/ &["?", "<", ">", "?", ">", "?", ">", ">", "<", ">"],
        /*-eps*/ &["?", "<", ">", "<", "?", "<", "?", "<", "<", ">"],
        /*  0 */ &["?", "<", ">", "<", ">", "<", ">", "=", "<", ">"],
        /*  1 */ &["?", "<", ">", "?", ">", ">", ">", ">", "=", ">"],
        /* -1 */ &["?", "<", ">", "<", "?", "<", "<", "<", "<", "="],
    ];
    for &n in &ns_huge() {
        with_n(n, || {
            for (i, &sa) in STATES.iter().enumerate() {
                for (j, &sb) in STATES.iter().enumerate() {
                    let got = sym(nfm::cmp(&mk(sa), &mk(sb)));
                    assert_eq!(got, g[i][j], "cmp({}, {}) at N={}", sa, sb, n);
                }
            }
        });
    }
}

#[test]
fn eq_semantics() {
    for &n in &ns_huge() {
        with_n(n, || {
            // NaN, Any, Inf and Eps never equal anything - not even
            // themselves (each stands for a different unknown magnitude)
            for &s in &["nan", "+any", "-any", "+inf", "-inf", "+eps", "-eps"] {
                assert!(!nfm::eq(&mk(s), &mk(s)), "eq({s}, {s}) at N={n}");
            }
            for &s in &["0", "1", "-1"] {
                assert!(nfm::eq(&mk(s), &mk(s)), "eq({s}, {s}) at N={n}");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// unary operations over all special states
// ---------------------------------------------------------------------------

#[test]
fn unary_special_tables() {
    type UFn = fn(&[Limb]) -> Vec<Limb>;
    // states: nan, +inf, -inf, +any, -any, +eps, -eps, 0
    let specials: [&str; 8] = ["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0"];
    let table: &[(&str, UFn, [&str; 8])] = &[
        ("neg", |a| nfm::neg(a), ["nan", "-inf", "+inf", "-any", "+any", "-eps", "+eps", "0"]),
        ("abs", |a| nfm::abs(a), ["nan", "+inf", "+inf", "+any", "+any", "+eps", "+eps", "0"]),
        ("square", |a| nfm::mul(a, a), ["nan", "+inf", "+inf", "+any", "+any", "+eps", "+eps", "0"]),
        ("sqrt", |a| real::sqrt(a), ["nan", "+inf", "nan", "+any", "nan", "+eps", "nan", "0"]),
        ("exp", |a| real::exp(a), ["nan", "+inf", "+eps", "+any", "+any", "1", "1", "1"]),
        ("ln", |a| real::ln(a), ["nan", "+inf", "nan", "nan", "nan", "-inf", "nan", "-inf"]),
        ("log2", |a| real::log2(a), ["nan", "+inf", "nan", "nan", "nan", "-inf", "nan", "-inf"]),
        ("log10", |a| real::log10(a), ["nan", "+inf", "nan", "nan", "nan", "-inf", "nan", "-inf"]),
        ("floor", |a| nfm::floor(a), ["nan", "+inf", "-inf", "nan", "-any", "0", "-1", "0"]),
        ("ceil", |a| nfm::ceil(a), ["nan", "+inf", "-inf", "+any", "nan", "1", "0", "0"]),
        ("round", |a| nfm::round(a), ["nan", "+inf", "-inf", "nan", "nan", "0", "0", "0"]),
        ("trunc", |a| nfm::trunc(a), ["nan", "+inf", "-inf", "nan", "nan", "0", "0", "0"]),
        ("sin", |a| real::sin(a), ["nan", "nan", "nan", "nan", "nan", "+eps", "-eps", "0"]),
        ("cos", |a| real::cos(a), ["nan", "nan", "nan", "nan", "nan", "1", "1", "1"]),
        ("tan", |a| real::tan(a), ["nan", "nan", "nan", "nan", "nan", "+eps", "-eps", "0"]),
        ("asin", |a| real::asin(a), ["nan", "nan", "nan", "nan", "nan", "+eps", "-eps", "0"]),
        ("atan", |a| real::atan(a), ["nan", ".", ".", "+any", "-any", "+eps", "-eps", "0"]),
        ("sinh", |a| real::sinh(a), ["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0"]),
        ("cosh", |a| real::cosh(a), ["nan", "+inf", "+inf", "+any", "+any", "1", "1", "1"]),
        ("tanh", |a| real::tanh(a), ["nan", "1", "-1", "+any", "-any", "+eps", "-eps", "0"]),
        ("asinh", |a| real::asinh(a), ["nan", "+inf", "-inf", "+any", "-any", "+eps", "-eps", "0"]),
        ("acosh", |a| real::acosh(a), ["nan", "+inf", "nan", "nan", "nan", "nan", "nan", "nan"]),
        ("atanh", |a| real::atanh(a), ["nan", "nan", "nan", "nan", "nan", "+eps", "-eps", "0"]),
    ];
    for &n in &ns_huge() {
        with_n(n, || {
            for (name, f, wants) in table {
                for (i, &s) in specials.iter().enumerate() {
                    let want = wants[i];
                    if want == "." {
                        continue;
                    }
                    let got = st(&f(&mk(s)));
                    assert_eq!(got, want, "{}({}) at N={}", name, s, n);
                }
            }
        });
    }
}

#[test]
fn atan_inf_is_half_pi() {
    for &n in &[2u32, 3, 6, 7, 8] {
        with_n(n, || {
            // atan rounds pi/2 once; div(pi(), 2) rounds twice - allow 1 ulp
            let half_pi = nfm::div(&real::pi(), &mk("2"));
            let close = |a: &[Limb], b: &[Limb]| {
                let d = nfm::abs(&nfm::sub(a, b));
                nfm::is_zero(&d)
                    || nfm::is_eps(&d)
                    || nfm::cmp(&d, &nfm::min_pos()) != Some(std::cmp::Ordering::Greater)
            };
            assert!(close(&real::atan(&mk("+inf")), &half_pi), "atan(+inf) N={}", n);
            assert!(
                close(&real::atan(&mk("-inf")), &nfm::neg(&half_pi)),
                "atan(-inf) N={}",
                n
            );
        });
    }
}

#[test]
fn atan2_special_cases() {
    for &n in &ns_huge() {
        with_n(n, || {
            let a2 = |y: &str, x: &str| st(&real::atan2(&mk(y), &mk(x)));
            assert_eq!(a2("0", "0"), "nan");
            assert_eq!(a2("nan", "1"), "nan");
            assert_eq!(a2("1", "nan"), "nan");
            assert_eq!(a2("+inf", "+inf"), "nan");
            assert_eq!(a2("+eps", "+eps"), "nan");
            // Any rows/cols
            assert_eq!(a2("+any", "1"), "+any");
            assert_eq!(a2("+any", "-1"), "+any");
            assert_eq!(a2("-any", "1"), "-any");
            assert_eq!(a2("-any", "+inf"), "-any");
            assert_eq!(a2("+any", "+any"), "+any");
            assert_eq!(a2("1", "+any"), "+any");
            assert_eq!(a2("-1", "-any"), "-any");
            assert_eq!(a2("0", "+any"), "0");
            // axes
            assert_eq!(a2("0", "1"), "0");
            assert_eq!(a2("1", "+inf"), "+eps");
            assert_eq!(a2("-1", "+inf"), "-eps");
            assert_eq!(a2("+eps", "1"), "+eps");
            // pi-valued results need the pi constant: series work, so only
            // at the small precisions (dense math at huge N is quadratic)
            if !is_huge(n) {
                let pi_s = st(&real::pi());
                assert_eq!(a2("0", "-any"), pi_s.clone());
                assert_eq!(a2("0", "-1"), pi_s.clone());
                assert_eq!(a2("+eps", "-1"), pi_s.clone());
            }
        });
    }
}

// ---------------------------------------------------------------------------
// all 16 flag nibbles: decoding, canonicalization, bit-string round trips
// ---------------------------------------------------------------------------

/// Build a bit string "ffff-i...i-f...f" for the current N.
fn bit_str(flags: &str, int_val: u64) -> String {
    let ib = nfm::frac_bits(); // integer and fraction widths are both 2^N
    let fb = nfm::frac_bits();
    let mut out = String::with_capacity(4 + ib + fb + 2);
    out.push_str(flags);
    out.push('-');
    for k in (0..ib).rev() {
        out.push(if k < 63 && (int_val >> k) & 1 == 1 { '1' } else { '0' });
    }
    out.push('-');
    for _ in 0..fb {
        out.push('0');
    }
    out
}

#[test]
fn all_16_flag_nibbles_decode() {
    // (flags, zero-payload state, one-payload state)
    let cases: [(&str, &str, &str); 16] = [
        ("0000", "0", "1"),
        ("0001", "0", "-1"), // -0 canonicalizes to +0
        ("0010", "+eps", "+eps"),
        ("0011", "-eps", "-eps"),
        ("0100", "+inf", "+inf"),
        ("0101", "-inf", "-inf"),
        ("0110", "+any", "+any"),
        ("0111", "-any", "-any"),
        ("1000", "nan", "nan"),
        ("1001", "nan", "nan"),
        ("1010", "nan", "nan"),
        ("1011", "nan", "nan"),
        ("1100", "nan", "nan"),
        ("1101", "nan", "nan"),
        ("1110", "nan", "nan"),
        ("1111", "nan", "nan"),
    ];
    for &n in &ns_huge() {
        with_n(n, || {
            for &(flags, want0, want1) in &cases {
                let s0 = bit_str(flags, 0);
                let v0 = nfm::from_bit_string(&s0).unwrap();
                assert_eq!(st(&v0), want0, "flags {} payload 0 at N={}", flags, n);
                let s1 = bit_str(flags, 1);
                let v1 = nfm::from_bit_string(&s1).unwrap();
                assert_eq!(st(&v1), want1, "flags {} payload 1 at N={}", flags, n);
            }
        });
    }
}

#[test]
fn bit_string_roundtrips() {
    for &n in &ns_huge() {
        with_n(n, || {
            for &s in &STATES {
                let v = mk(s);
                let bits = nfm::to_bit_string(&v);
                let back = nfm::from_bit_string(&bits).unwrap();
                assert_eq!(st(&back), st(&v), "bit roundtrip {} at N={}", s, n);
                // canonical printing is stable
                assert_eq!(nfm::to_bit_string(&back), bits, "canonical {} at N={}", s, n);
            }
        });
    }
}

#[test]
fn bit_string_exact_forms() {
    with_n(0, || {
        // N=0: 6 logical bits, "ffff-i-f"
        assert_eq!(nfm::to_bit_string(&mk("0")), "0000-0-0");
        assert_eq!(nfm::to_bit_string(&mk("1")), "0000-1-0");
        assert_eq!(nfm::to_bit_string(&mk("-1")), "0001-1-0");
        assert_eq!(nfm::to_bit_string(&mk("0.5")), "0000-0-1");
        assert_eq!(nfm::to_bit_string(&mk("1.5")), "0000-1-1");
        assert_eq!(nfm::to_bit_string(&mk("+inf")), "0100-0-0");
        assert_eq!(nfm::to_bit_string(&mk("-inf")), "0101-0-0");
        assert_eq!(nfm::to_bit_string(&mk("+eps")), "0010-0-0");
        assert_eq!(nfm::to_bit_string(&mk("+any")), "0110-0-0");
        assert_eq!(nfm::to_bit_string(&mk("-any")), "0111-0-0");
        assert_eq!(nfm::to_bit_string(&nfm::nan()), "1000-0-0");
        // parses with or without separators
        assert_eq!(st(&nfm::from_bit_string("0000-1-1").unwrap()), "1.5");
        assert_eq!(st(&nfm::from_bit_string("000011").unwrap()), "1.5");
    });
    with_n(1, || {
        // N=1: the user's "0000-00-00" notation (8 logical bits)
        assert_eq!(nfm::to_bit_string(&mk("0")), "0000-00-00");
        assert_eq!(st(&nfm::from_bit_string("0000-00-00").unwrap()), "0");
        assert_eq!(nfm::to_bit_string(&mk("-2")), "0001-10-00");
        assert_eq!(st(&nfm::from_bit_string("0001-10-00").unwrap()), "-2");
        assert_eq!(nfm::to_bit_string(&mk("3.75")), "0000-11-11");
    });
    with_n(3, || {
        // the specification example: 20 bits, -2.0
        assert_eq!(nfm::to_bit_string(&mk("-2")), "0001-00000010-00000000");
        assert_eq!(st(&nfm::from_bit_string("0001-00000010-00000000").unwrap()), "-2");
        assert_eq!(st(&nfm::from_bit_string("0000-00000000-00000000").unwrap()), "0");
    });
}

#[test]
fn negative_zero_canonicalizes() {
    for &n in &ns_huge() {
        with_n(n, || {
            let v = nfm::from_bit_string(&bit_str("0001", 0)).unwrap();
            assert!(nfm::is_zero(&v) && !nfm::is_neg(&v), "-0 -> +0 at N={}", n);
            let w = nfm::sub(&mk("1"), &mk("1"));
            assert!(nfm::is_zero(&w) && !nfm::is_neg(&w), "1-1 -> +0 at N={}", n);
        });
    }
}

// ---------------------------------------------------------------------------
// string parsing / printing of the special states
// ---------------------------------------------------------------------------

#[test]
fn parse_print_specials() {
    for &n in &ns_huge() {
        with_n(n, || {
            let cases: &[(&str, &str)] = &[
                ("nan", "nan"),
                ("inf", "+inf"),
                ("-inf", "-inf"),
                ("+inf", "+inf"),
                ("eps", "+eps"),
                ("-eps", "-eps"),
                ("infsmall", "+eps"),
                ("-infsmall", "-eps"),
                ("any", "+any"),
                ("-any", "-any"),
                ("+any", "+any"),
                ("0", "0"),
                ("-0", "0"),
                ("zero", "0"),
                ("-zero", "0"),
                ("+ZERO", "0"),
            ];
            for &(inp, want) in cases {
                let v = nfm::from_str(inp).unwrap();
                assert_eq!(st(&v), want, "parse '{}' at N={}", inp, n);
            }
            // display strings
            assert_eq!(nfm::to_string_digits(&mk("+any"), None), "+Any");
            assert_eq!(nfm::to_string_digits(&mk("-any"), None), "-Any");
            assert_eq!(nfm::to_string_digits(&mk("+inf"), None), "+Inf");
            assert_eq!(nfm::to_string_digits(&mk("+eps"), None), "+InfSmall");
            assert_eq!(nfm::to_string_digits(&nfm::nan(), None), "NaN");
        });
    }
}

#[test]
fn value_string_roundtrips() {
    // shortest-form printing must parse back to the identical value
    for &n in &ns_huge() {
        with_n(n, || {
            for &s in &["0.5", "1", "-1", "1.5", "-1.5", "0"] {
                let v = mk(s);
                let printed = nfm::to_string_digits(&v, None);
                let back = nfm::from_str(&printed).unwrap();
                assert!(nfm::eq(&v, &back), "roundtrip {} at N={}", s, n);
            }
            // extremes: largest finite and smallest positive.  Their decimal
            // expansions need ~F*log10(2) digits, so at huge N they are
            // round-tripped through bit strings instead of decimal.
            let max = nfm::max_value();
            let min = nfm::min_pos();
            if is_huge(n) {
                for v in [&max, &min] {
                    let back = nfm::from_bit_string(&nfm::to_bit_string(v)).unwrap();
                    assert!(nfm::eq(v, &back), "extreme bit roundtrip at N={}", n);
                }
            } else {
                let back = nfm::from_str(&nfm::to_string_digits(&max, None)).unwrap();
                assert!(nfm::eq(&max, &back), "max roundtrip at N={}", n);
                let back = nfm::from_str(&nfm::to_string_digits(&min, None)).unwrap();
                assert!(nfm::eq(&min, &back), "min roundtrip at N={}", n);
            }
        });
    }
}

#[test]
fn overflow_underflow_boundaries() {
    for &n in &ns_huge() {
        with_n(n, || {
            let max = nfm::max_value();
            let min = nfm::min_pos();
            // max + min overflows to +Inf; -max - min to -Inf
            assert_eq!(st(&nfm::add(&max, &min)), "+inf", "overflow at N={}", n);
            assert_eq!(st(&nfm::sub(&nfm::neg(&max), &min)), "-inf", "-overflow at N={}", n);
            // squaring the smallest positive underflows to +Eps for N >= 1;
            // at N=0 ties-away rounding keeps min*min = 0.25 alive as 0.5
            if n == 0 {
                assert_eq!(st(&nfm::mul(&min, &min)), "0.5", "N=0 tie-away");
            } else {
                assert_eq!(st(&nfm::mul(&min, &min)), "+eps", "underflow at N={}", n);
                assert_eq!(st(&nfm::mul(&nfm::neg(&min), &min)), "-eps");
            }
            if representable("2") {
                // max * 2 overflows
                assert_eq!(st(&nfm::mul(&max, &mk("2"))), "+inf");
            }
            // 1 / max underflows only when max > 1/min; always fine to check state
            let inv = nfm::div(&mk("1"), &max);
            assert!(
                st(&inv) == "+eps" || !nfm::is_neg(&inv),
                "1/max positive at N={}",
                n
            );
        });
    }
}

// ---------------------------------------------------------------------------
// complex layer: Any propagation
// ---------------------------------------------------------------------------

#[test]
fn complex_any_propagation() {
    for &n in &ns_huge() {
        with_n(n, || {
            let cs = |z: &[Limb]| nc::to_string_digits(z, None);
            let anyp = nc::from_re(&mk("+any"));
            let inf = nc::from_re(&mk("+inf"));
            let eps = nc::from_re(&mk("+eps"));
            // |x + Any*i| = +Any, |Inf + Any*i| = +Inf
            let z1 = nc::make(&mk("1"), &mk("+any"));
            assert_eq!(st(&nc::abs(&z1)), "+any", "abs(1+any i) N={}", n);
            let z2 = nc::make(&mk("+inf"), &mk("+any"));
            assert_eq!(st(&nc::abs(&z2)), "+inf", "abs(inf+any i) N={}", n);
            // real-axis algebra through the complex ops
            assert_eq!(cs(&nc::mul(&inf, &eps)), "(+Any)", "inf*eps N={}", n);
            assert_eq!(cs(&nc::div(&inf, &inf)), "(+Any)", "inf/inf N={}", n);
            assert_eq!(cs(&nc::div(&eps, &eps)), "(+Any)", "eps/eps N={}", n);
            assert_eq!(cs(&nc::mul(&anyp, &anyp)), "(+Any)", "any*any N={}", n);
            assert_eq!(cs(&nc::add(&anyp, &anyp)), "(+Any)", "any+any N={}", n);
            assert_eq!(cs(&nc::sub(&anyp, &anyp)), "(NaN)", "any-any N={}", n);
            assert_eq!(cs(&nc::exp(&anyp)), "(+Any)", "exp(any) N={}", n);
            assert_eq!(cs(&nc::sqrt(&anyp)), "(+Any)", "sqrt(any) N={}", n);
            assert_eq!(cs(&nc::ln(&anyp)), "(NaN)", "ln(any) N={}", n);
            // i * Any stays Any on the imaginary axis
            let rot = nc::mul_i(&anyp);
            assert_eq!(st(nc::re(&rot)), "0");
            assert_eq!(st(nc::im(&rot)), "+any");
        });
    }
}

// ---------------------------------------------------------------------------
// the same algebra end-to-end through the parser
// ---------------------------------------------------------------------------

#[test]
fn expr_special_algebra() {
    for &n in &ns_huge() {
        with_n(n, || {
            let ev = |s: &str| nc::to_string_digits(&expr::eval_str(s).unwrap(), None);
            assert_eq!(ev("inf*eps"), "(+Any)", "N={}", n);
            assert_eq!(ev("-inf*eps"), "(-Any)", "N={}", n);
            assert_eq!(ev("eps*inf"), "(+Any)", "N={}", n);
            assert_eq!(ev("inf/inf"), "(+Any)", "N={}", n);
            assert_eq!(ev("-inf/inf"), "(-Any)", "N={}", n);
            assert_eq!(ev("eps/eps"), "(+Any)", "N={}", n);
            assert_eq!(ev("eps/-eps"), "(-Any)", "N={}", n);
            assert_eq!(ev("any"), "(+Any)", "N={}", n);
            assert_eq!(ev("-any"), "(-Any)", "N={}", n);
            assert_eq!(ev("any*any"), "(+Any)", "N={}", n);
            assert_eq!(ev("any*-1"), "(-Any)", "N={}", n);
            assert_eq!(ev("any+any"), "(+Any)", "N={}", n);
            assert_eq!(ev("any-any"), "(NaN)", "N={}", n);
            assert_eq!(ev("any+1"), "(+Any)", "N={}", n);
            assert_eq!(ev("any-1"), "(NaN)", "N={}", n);
            assert_eq!(ev("inf-inf"), "(NaN)", "N={}", n);
            assert_eq!(ev("eps-eps"), "(NaN)", "N={}", n);
            assert_eq!(ev("0*inf"), "(NaN)", "N={}", n);
            assert_eq!(ev("0*any"), "(NaN)", "N={}", n);
            assert_eq!(ev("0*eps"), "0", "N={}", n);
            assert_eq!(ev("1/0"), "(NaN)", "N={}", n);
            assert_eq!(ev("0/0"), "(NaN)", "N={}", n);
            assert_eq!(ev("inf+inf"), "(+Inf)", "N={}", n);
            assert_eq!(ev("eps+eps"), "(+InfSmall)", "N={}", n);
            assert_eq!(ev("any^0"), "1", "N={}", n);
            assert_eq!(ev("1^any"), "(NaN)", "N={}", n); // like 1^Inf
            assert_eq!(ev("1^eps"), "1", "N={}", n);
            assert_eq!(ev("0^any"), "0", "N={}", n);
            assert_eq!(ev("0^-any"), "(NaN)", "N={}", n);
            assert_eq!(ev("inf^eps"), "(+Any)", "N={}", n);
            assert_eq!(ev("eps^eps"), "(+Any)", "N={}", n);
            assert_eq!(ev("sqrt(any)"), "(+Any)", "N={}", n);
            assert_eq!(ev("sqrt(-any)"), "(+Any)i", "N={}", n); // principal branch
            if !is_huge(n) {
                // Newton sqrt of a normal value: quadratic, small N only
                assert_eq!(ev("sqrt(-1)"), "i", "N={}", n);
            }
            assert_eq!(ev("exp(any)"), "(+Any)", "N={}", n);
            assert_eq!(ev("ln(any)"), "(NaN)", "N={}", n);
            assert_eq!(ev("atan(any)"), "(+Any)", "N={}", n);
            assert_eq!(ev("abs(-any)"), "(+Any)", "N={}", n);
            assert_eq!(ev("abs(any*i)"), "(+Any)", "N={}", n);
            // root() with special/complex k falls back to a^(1/k)
            assert_eq!(ev("root(2, 0)"), "(NaN)", "N={}", n);
            if representable("2") {
                assert_eq!(ev("root(2, eps)"), "(+Inf)", "N={}", n);
                assert_eq!(ev("root(2, inf)"), "1", "N={}", n);
                assert_eq!(ev("root(2, any)"), "(+Any)", "N={}", n);
            }
        });
    }
}

#[test]
fn expr_numeric_all_n() {
    // plain arithmetic that fits every N (max at N=0 is 1.5)
    for &n in &ns_huge() {
        with_n(n, || {
            let ev = |s: &str| nc::to_string_digits(&expr::eval_str(s).unwrap(), None);
            assert_eq!(ev("1"), "1");
            assert_eq!(ev("0.5+0.5"), "1");
            assert_eq!(ev("1.5-0.5"), "1");
            assert_eq!(ev("0.5*1"), "0.5");
            assert_eq!(ev("1/1"), "1");
            assert_eq!(ev("1*i*i"), "-1");
            assert_eq!(ev("abs(i)"), "1");
            assert_eq!(ev("re(0.5+i)"), "0.5");
            assert_eq!(ev("im(0.5+i)"), "1");
            assert_eq!(ev("conj(i)"), "-i");
        });
    }
    for &n in &[2u32, 3, 6, 7, 8, 24] {
        with_n(n, || {
            let ev = |s: &str| nc::to_string_digits(&expr::eval_str(s).unwrap(), None);
            assert_eq!(ev("2+2*2"), "6");
            assert_eq!(ev("(1+2i)*(3-1i)"), "5+5i");
            assert_eq!(ev("2^3"), "8");
            assert_eq!(ev("2**3"), "8");
            assert_eq!(ev("floor(2.5)"), "2");
            assert_eq!(ev("ceil(2.5)"), "3");
            assert_eq!(ev("round(2.5)"), "3");
            assert_eq!(ev("trunc(-2.5)"), "-2");
            assert_eq!(ev("floor(-2.5)"), "-3");
            if !is_huge(n) {
                // Newton roots of normal values: small N only
                assert_eq!(ev("root(8,3)"), "2");
                assert_eq!(ev("sqrt(4)"), "2");
            }
        });
    }
}

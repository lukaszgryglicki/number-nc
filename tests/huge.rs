//! Huge-precision stress tests at N = 24.
//!
//! N = 24 means F = 2^24 = 16 777 216 bits on each side of the point:
//! 2*(2^24)+4 = 33 554 436 logical bits (~4 MB) per number.  Everything
//! here runs the full-width representation end to end: construction,
//! exact sparse arithmetic across the whole mantissa, giant-integer
//! decimal I/O, deep fractions down to 2^-16777216, bit-string I/O of
//! 33.5-million-character strings, overflow/underflow at the real
//! boundaries, complex arithmetic, and the expression parser.
//!
//! Dense transcendental math (pi, exp of normals, Newton roots) is
//! quadratic in the width and is deliberately not exercised here; its
//! correctness is covered at N in {0, 1, 2, 3, 6, 7, 8} by the other
//! suites, which prove the same code paths that N = 24 merely widens.

use nf::nf::{self as nfm, with_n};
use nf::{expr, nc, Limb};

const N: u32 = 24;
const F: usize = 1 << 24; // 16_777_216

/// `(2^100 + 3) * (2^100 - 3) = 2^200 - 9`, exactly.
const P200_MINUS_9: &str =
    "1606938044258990275541962092341162602522202993782792835301367";

/// `2^1000`, exactly (302 digits).
const P1000: &str = "10715086071862673209484250490600018105614048117055336074437503883703510511249361224931983788156958581275946729175531468251871452856923140435984577574698574803934567774824230985421074605062371141877954182153046474983581941267398767559165543946077062914571196477686542167660429831652624386837205668069376";

/// The 100 exact decimal fraction digits of `2^-100` (i.e. 5^100, zero-padded).
const FRAC100: &str =
    "0000000000000000000000000000007888609052210118054117285652827862296732064351090230047702789306640625";

/// Bit string for `sign * 2^e` at the current N ("ffff-int-frac" layout,
/// int and frac both MSB-first).  `e` may be negative (fraction bit).
fn pow2_bits(neg: bool, e: i64) -> String {
    let ib = nfm::frac_bits();
    let fb = nfm::frac_bits();
    let mut s = String::with_capacity(4 + ib + fb + 2);
    s.push_str(if neg { "0001" } else { "0000" });
    s.push('-');
    for k in (0..ib as i64).rev() {
        s.push(if k == e { '1' } else { '0' });
    }
    s.push('-');
    for k in 1..=fb as i64 {
        s.push(if -k == e { '1' } else { '0' });
    }
    s
}

fn p2(e: i64) -> Vec<Limb> {
    nfm::from_bit_string(&pow2_bits(false, e)).unwrap()
}

#[test]
fn sizes_and_layout_at_24() {
    with_n(N, || {
        assert_eq!(nfm::frac_bits(), F);
        assert_eq!(nfm::total_bits(), 2 * F);
        assert_eq!(nfm::logical_bits(), 2 * F + 4); // 33_554_436
        assert_eq!(nfm::mant_limbs(), 2 * F / 64); // 524_288 limbs
        // a bit string spans the full logical width plus two separators
        let bits = nfm::to_bit_string(&nfm::zero());
        assert_eq!(bits.len(), 4 + 2 * F + 2);
        assert!(bits.starts_with("0000-"));
    });
}

#[test]
fn giant_integer_decimal_roundtrip() {
    with_n(N, || {
        // 2^1000: exact 302-digit decimal both ways
        let v = expr::eval_str("2^1000").unwrap();
        let s = nc::to_string_digits(&v, None);
        assert_eq!(s, P1000);
        let back = nfm::from_str(&s).unwrap();
        assert!(nfm::eq(nc::re(&v), &back));
        // 2^100000: a 30103-digit integer, still exact both ways
        let v = expr::eval_str("2^100000").unwrap();
        let s = nc::to_string_digits(&v, None);
        assert_eq!(s.len(), 30_103);
        assert!(s.starts_with("99900209") && s.ends_with("109376"), "2^100000 digits");
        let back = nfm::from_str(&s).unwrap();
        assert!(nfm::eq(nc::re(&v), &back));
        // and it equals 2^100000 built bit-by-bit
        assert!(nfm::eq(nc::re(&v), &p2(100_000)));
    });
}

#[test]
fn exact_sparse_arithmetic_full_width() {
    with_n(N, || {
        // a = 2^8000000 + 2^-8000000 spans almost the whole 33.5M-bit width
        let a = nfm::add(&p2(8_000_000), &p2(-8_000_000));
        let b = p2(4_000_000);
        // exact identities across the full mantissa
        assert!(nfm::eq(&nfm::sub(&nfm::add(&a, &b), &b), &a), "(a+b)-b == a");
        assert!(nfm::eq(&nfm::mul(&a, &nfm::one()), &a), "a*1 == a");
        assert!(nfm::eq(&nfm::div(&a, &nfm::one()), &a), "a/1 == a");
        let two = nfm::from_str("2").unwrap();
        assert!(nfm::eq(&nfm::div(&nfm::mul(&a, &two), &two), &a), "(a*2)/2 == a");
        assert!(nfm::eq(&nfm::neg(&nfm::neg(&a)), &a), "-(-a) == a");
        assert!(nfm::eq(&nfm::abs(&nfm::neg(&a)), &a), "|-a| == a");
        // floor/frac split is exact
        assert!(nfm::eq(&nfm::floor(&a), &p2(8_000_000)));
        assert!(nfm::eq(&nfm::sub(&a, &nfm::floor(&a)), &p2(-8_000_000)));
        // doubling walks bits across limb boundaries exactly
        assert!(nfm::eq(&nfm::add(&p2(-8_000_000), &p2(-8_000_000)), &p2(-7_999_999)));
        assert!(nfm::eq(&nfm::mul(&p2(1_000_000), &p2(1_000_000)), &p2(2_000_000)));
        assert!(nfm::eq(&nfm::div(&p2(1_000_000), &p2(2_000_000)), &p2(-1_000_000)));
        // comparison sees the single low bit 16.7M bits down
        assert_eq!(
            nfm::cmp(&a, &p2(8_000_000)),
            Some(std::cmp::Ordering::Greater),
            "a > floor(a) by one 2^-8000000 bit"
        );
    });
}

#[test]
fn deep_fraction_boundaries() {
    with_n(N, || {
        let min = nfm::min_pos(); // 2^-16777216
        assert!(nfm::eq(&min, &p2(-(F as i64))), "min_pos is the lowest frac bit");
        // bit string: 33.5M chars, exactly one '1', roundtrips
        let bits = nfm::to_bit_string(&min);
        assert_eq!(bits.len(), 4 + 2 * F + 2);
        assert_eq!(bits.chars().filter(|&c| c == '1').count(), 1);
        assert!(bits.ends_with('1'));
        assert!(nfm::eq(&nfm::from_bit_string(&bits).unwrap(), &min));
        // arithmetic at the very bottom of the range
        assert!(nfm::eq(&nfm::add(&min, &min), &p2(-(F as i64) + 1)), "min+min");
        assert!(nfm::is_eps(&nfm::mul(&min, &min)), "min^2 underflows to +Eps");
        assert!(nfm::is_inf(&nfm::div(&nfm::one(), &min)), "1/min overflows to +Inf");
        assert!(!nfm::is_neg(&nfm::div(&nfm::one(), &min)));
        // subtracting it from 1 borrows across 262144 limbs exactly
        let almost1 = nfm::sub(&nfm::one(), &min);
        assert!(!nfm::eq(&almost1, &nfm::one()));
        assert!(nfm::eq(&nfm::add(&almost1, &min), &nfm::one()));
    });
}

#[test]
fn max_value_boundaries_at_24() {
    with_n(N, || {
        let max = nfm::max_value();
        let min = nfm::min_pos();
        // the bit string is all ones in both mantissa halves
        let bits = nfm::to_bit_string(&max);
        assert_eq!(bits.len(), 4 + 2 * F + 2);
        assert!(bits.starts_with("0000-"));
        assert_eq!(bits.chars().filter(|&c| c == '1').count(), 2 * F);
        assert!(nfm::eq(&nfm::from_bit_string(&bits).unwrap(), &max));
        // boundary semantics
        assert!(nfm::is_inf(&nfm::add(&max, &min)), "max+min -> +Inf");
        assert!(nfm::is_inf(&nfm::mul(&max, &nfm::from_str("2").unwrap())), "2max -> +Inf");
        let z = nfm::sub(&max, &max);
        assert!(nfm::is_zero(&z) && !nfm::is_neg(&z), "max-max == +0");
        let inv = nfm::div(&nfm::one(), &max); // dense divisor, tiny quotient
        assert!(!nfm::is_neg(&inv) && !nfm::is_zero(&inv), "0 < 1/max");
        assert_eq!(nfm::cmp(&inv, &min), Some(std::cmp::Ordering::Equal), "1/max rounds to min");
    });
}

#[test]
fn long_decimal_io_at_24() {
    with_n(N, || {
        // (10^999 + 7) + 2^-100: a 1000-digit integer part and the exact
        // 100-digit fraction of 2^-100, together in one 1101-char literal
        let mut int_part = String::from("1");
        int_part.push_str(&"0".repeat(998));
        int_part.push('7');
        let lit = format!("{int_part}.{FRAC100}");
        let v = nfm::from_str(&lit).unwrap();
        // shortest-roundtrip printing reproduces the literal exactly
        assert_eq!(nfm::to_string_digits(&v, None), lit);
        // fixed-digit printing pads then trims back to the same string
        assert_eq!(nfm::to_string_digits(&v, Some(104)), lit);
        // and it decomposes exactly
        let int_only = nfm::from_str(&int_part).unwrap();
        assert!(nfm::eq(&nfm::floor(&v), &int_only));
        assert!(nfm::eq(&nfm::sub(&v, &int_only), &p2(-100)));
        // scientific notation parses at this scale too
        let sci = nfm::from_str("1e999").unwrap();
        assert!(nfm::eq(&nfm::sub(&int_only, &sci), &nfm::from_str("7").unwrap()));
    });
}

#[test]
fn complex_and_parser_at_24() {
    with_n(N, || {
        let ev = |s: &str| nc::to_string_digits(&expr::eval_str(s).unwrap(), None);
        // exact giant complex product: (2^100+3)(2^100-3) = 2^200 - 9
        assert_eq!(ev("(2^100+3)*(2^100-3)"), P200_MINUS_9);
        // (a+bi)(a-bi) = a^2 + b^2 with sparse giants, self-consistent
        // (note: `2^50*i`, since `2^50i` would parse as the complex power 2^(50i))
        assert_eq!(
            ev("(2^100 + 2^50*i) * (2^100 - 2^50*i)"),
            ev("2^200 + 2^100"),
            "conjugate product"
        );
        assert_eq!(ev("re(2^100+5i) - 2^100"), "0");
        assert_eq!(ev("im(2^100+5i)"), "5");
        assert_eq!(ev("conj(2^100+5i) + 5i - 2^100"), "0");
        assert_eq!(ev("(1+2i)*(3-1i)"), "5+5i");
        assert_eq!(ev("i^2"), "-1");
        assert_eq!(ev("2^-100 * 2^100"), "1");
        // special algebra still exact at this width
        assert_eq!(ev("inf*eps"), "(+Any)");
        assert_eq!(ev("inf/inf"), "(+Any)");
        assert_eq!(ev("inf-inf"), "(NaN)");
        assert_eq!(ev("0*inf"), "(NaN)");
        // deep-fraction decimal: 2^-100 prints its exact 100 digits
        let m100 = expr::eval_str("2^-100").unwrap();
        assert_eq!(nc::to_string_digits(&m100, None), format!("0.{}", FRAC100.trim_end_matches('0')));
    });
}

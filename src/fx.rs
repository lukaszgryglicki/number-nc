//! Internal **signed fixed-point working values** on raw limb arrays.
//!
//! Used by the transcendental-function engine: values are computed at a
//! *working scale* `s` (fraction bits), which is the NF fraction size plus
//! guard bits, then rounded once into the NF format at the end.
//!
//! A value is `(-1)^neg * mag / 2^s` where `s` is managed by the caller.
//! Multiplication/division here truncate toward zero - the guard bits absorb
//! those sub-ulp errors before the final rounding.

use crate::limb;
use crate::limb::Limb;
use std::cmp::Ordering;

/// Signed magnitude working value (scale is contextual).
#[derive(Clone, Debug, Default)]
pub struct Fx {
    pub neg: bool,
    pub mag: Vec<Limb>,
}

impl Fx {
    #[inline]
    pub fn zero() -> Fx {
        Fx { neg: false, mag: vec![] }
    }

    /// Build from sign + magnitude, normalizing zero to `+0`.
    pub fn new(neg: bool, mut mag: Vec<Limb>) -> Fx {
        limb::trim(&mut mag);
        if mag.is_empty() {
            Fx::zero()
        } else {
            Fx { neg, mag }
        }
    }

    /// Integer `v` at scale `s`: `v * 2^s`.
    pub fn from_u64(v: u64, s: usize) -> Fx {
        Fx::new(false, limb::shl(&limb::from_u64(v), s))
    }

    pub fn from_i64(v: i64, s: usize) -> Fx {
        Fx::new(v < 0, limb::shl(&limb::from_u64(v.unsigned_abs()), s))
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.mag.is_empty()
    }

    #[inline]
    pub fn bit_len(&self) -> usize {
        limb::bit_len(&self.mag)
    }

    pub fn neg(&self) -> Fx {
        Fx::new(!self.neg, self.mag.clone())
    }

    pub fn abs(&self) -> Fx {
        Fx::new(false, self.mag.clone())
    }

    /// Signed comparison.
    #[allow(clippy::should_implement_trait)] // deliberate inherent name; Fx is not Ord
    pub fn cmp(&self, o: &Fx) -> Ordering {
        match (self.is_zero(), o.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return if o.neg { Ordering::Greater } else { Ordering::Less },
            (false, true) => return if self.neg { Ordering::Less } else { Ordering::Greater },
            _ => {}
        }
        match (self.neg, o.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => limb::cmp(&self.mag, &o.mag),
            (true, true) => limb::cmp(&o.mag, &self.mag),
        }
    }

    pub fn cmp_abs(&self, o: &Fx) -> Ordering {
        limb::cmp(&self.mag, &o.mag)
    }

    /// Signed addition.
    pub fn add(&self, o: &Fx) -> Fx {
        if self.is_zero() {
            return o.clone();
        }
        if o.is_zero() {
            return self.clone();
        }
        if self.neg == o.neg {
            Fx::new(self.neg, limb::add(&self.mag, &o.mag))
        } else {
            match limb::cmp(&self.mag, &o.mag) {
                Ordering::Equal => Fx::zero(),
                Ordering::Greater => Fx::new(self.neg, limb::sub(&self.mag, &o.mag)),
                Ordering::Less => Fx::new(o.neg, limb::sub(&o.mag, &self.mag)),
            }
        }
    }

    pub fn sub(&self, o: &Fx) -> Fx {
        self.add(&o.neg())
    }

    /// `(a * b) >> s` truncated toward zero.
    pub fn mul_rs(&self, o: &Fx, s: usize) -> Fx {
        if self.is_zero() || o.is_zero() {
            return Fx::zero();
        }
        Fx::new(self.neg != o.neg, limb::shr(&limb::mul(&self.mag, &o.mag), s))
    }

    /// `(a << s) / b` truncated toward zero.  Panics on division by zero.
    pub fn div_s(&self, o: &Fx, s: usize) -> Fx {
        assert!(!o.is_zero(), "fx: division by zero");
        if self.is_zero() {
            return Fx::zero();
        }
        let num = limb::shl(&self.mag, s);
        Fx::new(self.neg != o.neg, limb::div_rem(&num, &o.mag).0)
    }

    pub fn mul_small(&self, m: u64) -> Fx {
        Fx::new(self.neg, limb::mul_small(&self.mag, m))
    }

    /// Divide by a small integer, truncated toward zero.
    pub fn div_small(&self, d: u64) -> Fx {
        Fx::new(self.neg, limb::div_rem_small(&self.mag, d).0)
    }

    pub fn shl(&self, k: usize) -> Fx {
        Fx::new(self.neg, limb::shl(&self.mag, k))
    }

    pub fn shr(&self, k: usize) -> Fx {
        Fx::new(self.neg, limb::shr(&self.mag, k))
    }

    /// `sqrt(value)` at scale `s`: `isqrt(mag << s)`.  Requires non-negative.
    pub fn sqrt_s(&self, s: usize) -> Fx {
        assert!(!self.neg || self.is_zero(), "fx: sqrt of negative");
        Fx::new(false, limb::isqrt(&limb::shl(&self.mag, s)))
    }

    /// Round the fixed-point value from scale `s` down to `keep` fraction
    /// bits (ties away from zero); result is a bare rounded magnitude.
    pub fn round_to(&self, s: usize, keep: usize) -> (bool, Vec<Limb>) {
        debug_assert!(s >= keep);
        (self.neg, limb::round_shr(&self.mag, s - keep))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = 64;

    fn approx(v: f64) -> Fx {
        // build from f64 for tests: v * 2^64
        let neg = v < 0.0;
        let a = v.abs();
        let hi = (a.trunc()) as u64;
        let lo = (a.fract() * (u64::MAX as f64 + 1.0)) as u64;
        Fx::new(neg, vec![lo, hi])
    }

    fn to_f64(v: &Fx) -> f64 {
        let m = &v.mag;
        let lo = *m.first().unwrap_or(&0) as f64 / 1.8446744073709552e19;
        let hi = *m.get(1).unwrap_or(&0) as f64;
        let a = hi + lo;
        if v.neg {
            -a
        } else {
            a
        }
    }

    #[test]
    fn signed_add_sub() {
        let a = approx(3.5);
        let b = approx(-5.25);
        assert!((to_f64(&a.add(&b)) + 1.75).abs() < 1e-12);
        assert!((to_f64(&a.sub(&b)) - 8.75).abs() < 1e-12);
        assert!((to_f64(&b.sub(&a)) + 8.75).abs() < 1e-12);
        assert!(a.sub(&a).is_zero());
    }

    #[test]
    fn mul_div() {
        let a = approx(3.5);
        let b = approx(-2.25);
        assert!((to_f64(&a.mul_rs(&b, S)) + 7.875).abs() < 1e-12);
        assert!((to_f64(&a.div_s(&b, S)) + 1.5555555555).abs() < 1e-9);
        assert!((to_f64(&a.mul_small(3)) - 10.5).abs() < 1e-12);
        assert!((to_f64(&a.div_small(7)) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn sqrt() {
        let a = approx(2.0);
        let s = a.sqrt_s(S);
        assert!((to_f64(&s) - std::f64::consts::SQRT_2).abs() < 1e-9);
    }

    #[test]
    fn cmp_signed() {
        use Ordering::*;
        assert_eq!(approx(-3.0).cmp(&approx(2.0)), Less);
        assert_eq!(approx(3.0).cmp(&approx(2.0)), Greater);
        assert_eq!(approx(-3.0).cmp(&approx(-2.0)), Less);
        assert_eq!(approx(0.0).cmp(&Fx::zero()), Equal);
    }
}

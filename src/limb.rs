//! Low-level arbitrary-size **unsigned** integer arithmetic on little-endian
//! arrays of 64-bit machine words ("limbs").
//!
//! This is the raw computational core: numbers here are just `&[u64]` /
//! `Vec<u64>` magnitudes (no sign, no scale).  Higher layers add fixed-point
//! scaling, signs and the NF special bits.
//!
//! All algorithms are chosen for speed on machine words:
//! * addition/subtraction: single-pass with carry/borrow,
//! * multiplication: schoolbook for small operands, Karatsuba above a
//!   threshold, chunked for very unbalanced operands,
//! * division: Knuth algorithm D (with `u128` two-word primitives),
//! * square root / k-th root: Newton iteration (sqrt is seeded from an `f64`
//!   estimate so only a handful of big-word iterations are needed).

use std::cmp::Ordering;

/// One machine word.
pub type Limb = u64;

/// Number of bits in one limb.
pub const LIMB_BITS: usize = 64;

/// Operand size (in limbs) above which multiplication switches to Karatsuba.
pub const KARATSUBA_THRESHOLD: usize = 32;

// ---------------------------------------------------------------------------
// basic helpers
// ---------------------------------------------------------------------------

/// Remove most-significant zero limbs in place.
#[inline]
pub fn trim(v: &mut Vec<Limb>) {
    while let Some(&0) = v.last() {
        v.pop();
    }
}

/// Return a trimmed copy of `a`.
#[inline]
pub fn trimmed(a: &[Limb]) -> Vec<Limb> {
    let mut v = a.to_vec();
    trim(&mut v);
    v
}

/// True if the magnitude is zero (all limbs zero or empty).
#[inline]
pub fn is_zero(a: &[Limb]) -> bool {
    a.iter().all(|&x| x == 0)
}

/// Effective length ignoring most-significant zero limbs.
#[inline]
pub fn eff_len(a: &[Limb]) -> usize {
    let mut l = a.len();
    while l > 0 && a[l - 1] == 0 {
        l -= 1;
    }
    l
}

/// Position of the highest set bit plus one (0 for zero).
#[inline]
pub fn bit_len(a: &[Limb]) -> usize {
    let l = eff_len(a);
    if l == 0 {
        0
    } else {
        (l - 1) * LIMB_BITS + (LIMB_BITS - a[l - 1].leading_zeros() as usize)
    }
}

/// Test bit `i` (bit 0 = least significant).
#[inline]
pub fn get_bit(a: &[Limb], i: usize) -> bool {
    let (l, b) = (i / LIMB_BITS, i % LIMB_BITS);
    l < a.len() && (a[l] >> b) & 1 == 1
}

/// Set bit `i`, growing the vector as needed.
pub fn set_bit(a: &mut Vec<Limb>, i: usize) {
    let (l, b) = (i / LIMB_BITS, i % LIMB_BITS);
    if a.len() <= l {
        a.resize(l + 1, 0);
    }
    a[l] |= 1 << b;
}

/// Compare two magnitudes (leading zero limbs are ignored).
pub fn cmp(a: &[Limb], b: &[Limb]) -> Ordering {
    let (la, lb) = (eff_len(a), eff_len(b));
    if la != lb {
        return la.cmp(&lb);
    }
    for i in (0..la).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    Ordering::Equal
}

/// Single-limb magnitude (empty for zero).
#[inline]
pub fn from_u64(v: u64) -> Vec<Limb> {
    if v == 0 {
        vec![]
    } else {
        vec![v]
    }
}

// ---------------------------------------------------------------------------
// addition / subtraction
// ---------------------------------------------------------------------------

/// `a + b`.
pub fn add(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut r = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for i in 0..long.len() {
        let (s1, c1) = long[i].overflowing_add(if i < short.len() { short[i] } else { 0 });
        let (s2, c2) = s1.overflowing_add(carry);
        r.push(s2);
        carry = (c1 as u64) + (c2 as u64);
    }
    if carry != 0 {
        r.push(carry);
    }
    r
}

/// `a += b` in place (grows `a` if needed).
pub fn add_in_place(a: &mut Vec<Limb>, b: &[Limb]) {
    if a.len() < b.len() {
        a.resize(b.len(), 0);
    }
    let mut carry = 0u64;
    for i in 0..a.len() {
        let bi = if i < b.len() { b[i] } else if carry == 0 { break } else { 0 };
        let (s1, c1) = a[i].overflowing_add(bi);
        let (s2, c2) = s1.overflowing_add(carry);
        a[i] = s2;
        carry = (c1 as u64) + (c2 as u64);
    }
    if carry != 0 {
        a.push(carry);
    }
}

/// `a += s` for a single limb `s`.
pub fn add_small_in_place(a: &mut Vec<Limb>, s: Limb) {
    let mut carry = s;
    let mut i = 0;
    while carry != 0 {
        if i == a.len() {
            a.push(carry);
            return;
        }
        let (v, c) = a[i].overflowing_add(carry);
        a[i] = v;
        carry = c as u64;
        i += 1;
    }
}

/// `a - b`; requires `a >= b` (checked in debug builds).
pub fn sub(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    debug_assert!(cmp(a, b) != Ordering::Less, "limb::sub underflow");
    let mut r = Vec::with_capacity(a.len());
    let mut borrow = 0u64;
    for i in 0..a.len() {
        let bi = if i < b.len() { b[i] } else { 0 };
        let (d1, o1) = a[i].overflowing_sub(bi);
        let (d2, o2) = d1.overflowing_sub(borrow);
        r.push(d2);
        borrow = (o1 as u64) + (o2 as u64);
    }
    debug_assert_eq!(borrow, 0);
    trim(&mut r);
    r
}

// ---------------------------------------------------------------------------
// shifts
// ---------------------------------------------------------------------------

/// `a << k`.
pub fn shl(a: &[Limb], k: usize) -> Vec<Limb> {
    let la = eff_len(a);
    if la == 0 {
        return vec![];
    }
    let (limbs, bits) = (k / LIMB_BITS, k % LIMB_BITS);
    let mut r = vec![0u64; la + limbs + 1];
    if bits == 0 {
        r[limbs..limbs + la].copy_from_slice(&a[..la]);
    } else {
        for i in 0..la {
            r[i + limbs] |= a[i] << bits;
            r[i + limbs + 1] |= a[i] >> (LIMB_BITS - bits);
        }
    }
    trim(&mut r);
    r
}

/// `a >> k` (truncating floor shift).
pub fn shr(a: &[Limb], k: usize) -> Vec<Limb> {
    let la = eff_len(a);
    let (limbs, bits) = (k / LIMB_BITS, k % LIMB_BITS);
    if limbs >= la {
        return vec![];
    }
    let n = la - limbs;
    let mut r = Vec::with_capacity(n);
    if bits == 0 {
        r.extend_from_slice(&a[limbs..la]);
    } else {
        for i in 0..n {
            let lo = a[i + limbs] >> bits;
            let hi = if i + limbs + 1 < la {
                a[i + limbs + 1] << (LIMB_BITS - bits)
            } else {
                0
            };
            r.push(lo | hi);
        }
    }
    trim(&mut r);
    r
}

/// `round(a / 2^k)` with ties rounded **away from zero** (up in magnitude):
/// shift right by `k` and add one if the highest dropped bit was set.
pub fn round_shr(a: &[Limb], k: usize) -> Vec<Limb> {
    if k == 0 {
        return trimmed(a);
    }
    let mut r = shr(a, k);
    if get_bit(a, k - 1) {
        add_small_in_place(&mut r, 1);
    }
    r
}

// ---------------------------------------------------------------------------
// multiplication
// ---------------------------------------------------------------------------

/// `a * m` for a single limb `m`.
pub fn mul_small(a: &[Limb], m: Limb) -> Vec<Limb> {
    let la = eff_len(a);
    if la == 0 || m == 0 {
        return vec![];
    }
    let mut r = Vec::with_capacity(la + 1);
    let mut carry = 0u128;
    for &w in &a[..la] {
        let t = w as u128 * m as u128 + carry;
        r.push(t as u64);
        carry = t >> 64;
    }
    if carry != 0 {
        r.push(carry as u64);
    }
    r
}

/// Schoolbook multiplication, O(la*lb).  Exposed for testing.
pub fn mul_schoolbook(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    let (la, lb) = (eff_len(a), eff_len(b));
    if la == 0 || lb == 0 {
        return vec![];
    }
    let mut r = vec![0u64; la + lb];
    for i in 0..la {
        let ai = a[i] as u128;
        if ai == 0 {
            continue;
        }
        let mut carry = 0u128;
        for j in 0..lb {
            let t = ai * b[j] as u128 + r[i + j] as u128 + carry;
            r[i + j] = t as u64;
            carry = t >> 64;
        }
        let mut k = i + lb;
        let mut c = carry as u64;
        while c != 0 {
            let (s, o) = r[k].overflowing_add(c);
            r[k] = s;
            c = o as u64;
            k += 1;
        }
    }
    trim(&mut r);
    r
}

/// `a * b`: schoolbook below [`KARATSUBA_THRESHOLD`], Karatsuba above it,
/// chunked recursion for very unbalanced operands.
pub fn mul(a: &[Limb], b: &[Limb]) -> Vec<Limb> {
    let (la, lb) = (eff_len(a), eff_len(b));
    let (a, b) = (&a[..la], &b[..lb]);
    if la == 0 || lb == 0 {
        return vec![];
    }
    let (short, long) = if la <= lb { (a, b) } else { (b, a) };
    if short.len() < KARATSUBA_THRESHOLD {
        return mul_schoolbook(short, long);
    }
    // Very unbalanced: multiply the long operand chunk by chunk so Karatsuba
    // always sees roughly balanced halves.
    if long.len() > 2 * short.len() {
        let cs = short.len();
        let mut r = vec![0u64; short.len() + long.len()];
        let mut off = 0;
        while off < long.len() {
            let end = (off + cs).min(long.len());
            let part = mul(short, &long[off..end]);
            // r[off..] += part
            let mut carry = 0u64;
            for (i, &p) in part.iter().enumerate() {
                let (s1, c1) = r[off + i].overflowing_add(p);
                let (s2, c2) = s1.overflowing_add(carry);
                r[off + i] = s2;
                carry = (c1 as u64) + (c2 as u64);
            }
            let mut k = off + part.len();
            while carry != 0 {
                let (s, o) = r[k].overflowing_add(carry);
                r[k] = s;
                carry = o as u64;
                k += 1;
            }
            off = end;
        }
        trim(&mut r);
        return r;
    }
    // Karatsuba: split both numbers at m limbs.
    let m = short.len() / 2;
    let (a0, a1) = (&short[..m], &short[m..]);
    let (b0, b1) = (&long[..m], &long[m..]);
    let z0 = mul(a0, b0);
    let z2 = mul(a1, b1);
    let sa = add(a0, a1);
    let sb = add(b0, b1);
    let mut z1 = mul(&sa, &sb);
    z1 = sub(&z1, &z0);
    z1 = sub(&z1, &z2);
    // result = z0 + z1 << (64*m) + z2 << (128*m)
    let mut r = vec![0u64; short.len() + long.len() + 1];
    let add_at = |r: &mut Vec<u64>, v: &[u64], off: usize| {
        let mut carry = 0u64;
        for (i, &p) in v.iter().enumerate() {
            let (s1, c1) = r[off + i].overflowing_add(p);
            let (s2, c2) = s1.overflowing_add(carry);
            r[off + i] = s2;
            carry = (c1 as u64) + (c2 as u64);
        }
        let mut k = off + v.len();
        while carry != 0 {
            let (s, o) = r[k].overflowing_add(carry);
            r[k] = s;
            carry = o as u64;
            k += 1;
        }
    };
    add_at(&mut r, &z0, 0);
    add_at(&mut r, &z1, m);
    add_at(&mut r, &z2, 2 * m);
    trim(&mut r);
    r
}

/// `a^e` by binary exponentiation.
pub fn pow(a: &[Limb], e: u64) -> Vec<Limb> {
    let mut result = vec![1u64];
    let mut base = trimmed(a);
    let mut e = e;
    if e == 0 {
        return result;
    }
    loop {
        if e & 1 == 1 {
            result = mul(&result, &base);
        }
        e >>= 1;
        if e == 0 {
            break;
        }
        base = mul(&base, &base);
    }
    result
}

/// `10^k` as a magnitude (square-and-multiply, subquadratic for huge `k`).
pub fn pow10(k: usize) -> Vec<Limb> {
    const CHUNK: usize = 19; // 10^19 fits in u64
    const P19: u64 = 10_000_000_000_000_000_000;
    let mut r = from_u64(10u64.pow((k % CHUNK) as u32));
    let mut base = from_u64(P19);
    let mut left = k / CHUNK;
    while left > 0 {
        if left & 1 == 1 {
            r = mul(&r, &base);
        }
        left >>= 1;
        if left > 0 {
            base = mul(&base, &base);
        }
    }
    r
}

// ---------------------------------------------------------------------------
// division
// ---------------------------------------------------------------------------

/// `(a / d, a % d)` for a single limb divisor `d != 0`.
pub fn div_rem_small(a: &[Limb], d: Limb) -> (Vec<Limb>, Limb) {
    assert!(d != 0, "division by zero");
    let la = eff_len(a);
    let mut q = vec![0u64; la];
    let mut rem = 0u128;
    for i in (0..la).rev() {
        let cur = (rem << 64) | a[i] as u128;
        q[i] = (cur / d as u128) as u64;
        rem = cur % d as u128;
    }
    trim(&mut q);
    (q, rem as u64)
}

/// `(a / b, a % b)` - Knuth algorithm D on 64-bit limbs.
///
/// Common trailing zero limbs of both operands are stripped first: with
/// `u = u'*B^t`, `v = v'*B^t` the floor quotient is unchanged and the
/// remainder is exactly `(u' mod v')*B^t`.  This makes division by
/// scaled-but-sparse operands (e.g. small integers at huge N) O(n).
///
/// Panics on division by zero.
pub fn div_rem(u: &[Limb], v: &[Limb]) -> (Vec<Limb>, Vec<Limb>) {
    let lv = eff_len(v);
    assert!(lv != 0, "division by zero");
    let lu = eff_len(u);
    if lu == 0 || cmp(u, v) == Ordering::Less {
        return (vec![], trimmed(u));
    }
    let tz_u = u.iter().take_while(|&&w| w == 0).count();
    let tz_v = v.iter().take_while(|&&w| w == 0).count();
    let tz = tz_u.min(tz_v).min(lu - 1).min(lv - 1);
    if tz > 0 {
        let (q, r) = div_rem(&u[tz..lu], &v[tz..lv]);
        if r.is_empty() {
            return (q, r);
        }
        let mut rs = vec![0u64; tz + r.len()];
        rs[tz..].copy_from_slice(&r);
        return (q, rs);
    }
    if lv == 1 {
        let (q, r) = div_rem_small(u, v[0]);
        return (q, from_u64(r));
    }
    // Normalize so the top limb of v has its high bit set.
    let s = v[lv - 1].leading_zeros() as usize;
    let vn = shl(&v[..lv], s);
    let mut un = shl(&u[..lu], s);
    debug_assert_eq!(vn.len(), lv);
    un.resize(lu + 1, 0); // one extra high limb for the algorithm
    let n = lv;
    let m = un.len() - 1 - n;
    let mut q = vec![0u64; m + 1];
    let b: u128 = 1u128 << 64;
    let vtop = vn[n - 1] as u128;
    let vsec = vn[n - 2] as u128;
    for j in (0..=m).rev() {
        let num = ((un[j + n] as u128) << 64) | un[j + n - 1] as u128;
        let mut qhat = num / vtop;
        let mut rhat = num - qhat * vtop;
        // Correct qhat down to at most one too large.
        while qhat >= b || qhat * vsec > ((rhat << 64) | un[j + n - 2] as u128) {
            qhat -= 1;
            rhat += vtop;
            if rhat >= b {
                break;
            }
        }
        // un[j..j+n+1] -= qhat * vn
        let mut carry = 0u128; // multiplication carry
        let mut borrow = 0i128; // subtraction borrow
        for i in 0..n {
            let p = qhat * vn[i] as u128 + carry;
            carry = p >> 64;
            let t = un[i + j] as i128 - (p as u64) as i128 - borrow;
            un[i + j] = t as u64;
            borrow = (t < 0) as i128;
        }
        let t = un[j + n] as i128 - carry as i128 - borrow;
        un[j + n] = t as u64;
        if t < 0 {
            // qhat was one too large: add back.
            qhat -= 1;
            let mut c = 0u64;
            for i in 0..n {
                let (s1, c1) = un[i + j].overflowing_add(vn[i]);
                let (s2, c2) = s1.overflowing_add(c);
                un[i + j] = s2;
                c = (c1 as u64) + (c2 as u64);
            }
            un[j + n] = un[j + n].wrapping_add(c);
        }
        q[j] = qhat as u64;
    }
    un.truncate(n);
    let r = shr(&un, s);
    trim(&mut q);
    (q, r)
}

// ---------------------------------------------------------------------------
// roots
// ---------------------------------------------------------------------------

/// Approximate value of the top (up to) 64 significant bits as `f64`,
/// together with the power-of-two shift that was applied:
/// `a ≈ mantissa * 2^shift`.
fn top_f64(a: &[Limb]) -> (f64, usize) {
    let bl = bit_len(a);
    if bl <= 64 {
        let l = eff_len(a);
        let v = if l == 0 { 0 } else { a[0] };
        return (v as f64, 0);
    }
    let sh = bl - 64;
    let top = shr(a, sh);
    ((top[0]) as f64, sh)
}

/// `floor(sqrt(a))` - Newton iteration seeded from an `f64` estimate.
pub fn isqrt(a: &[Limb]) -> Vec<Limb> {
    let la = eff_len(a);
    if la == 0 {
        return vec![];
    }
    if la == 1 {
        return from_u64(u64_isqrt(a[0]));
    }
    // Seed x0 >= floor(sqrt(a)) from the f64 of the top bits.
    let (mant, sh) = top_f64(a);
    let (half, odd) = (sh / 2, sh % 2);
    // a ≈ mant * 2^sh = (mant * 2^odd) * 2^(2*half)
    let g = (mant * if odd == 1 { 2.0 } else { 1.0 }).sqrt();
    // Round the guess up generously so we provably start >= sqrt(a):
    // f64 relative error << 2^-45.
    let gi = g as u64;
    let mut x = shl(&from_u64(gi.saturating_add((gi >> 45) + 2)), half);
    // Newton: x_{k+1} = (x + a/x) / 2, monotone decreasing while x > sqrt(a).
    loop {
        let (q, _) = div_rem(a, &x);
        let nx = shr(&add(&x, &q), 1);
        if cmp(&nx, &x) != Ordering::Less {
            break;
        }
        x = nx;
    }
    // Final correction (the loop lands within a couple of units).
    while cmp(&mul(&x, &x), a) == Ordering::Greater {
        x = sub(&x, &[1]);
    }
    loop {
        let x1 = add(&x, &[1]);
        if cmp(&mul(&x1, &x1), a) != Ordering::Greater {
            x = x1;
        } else {
            break;
        }
    }
    x
}

fn u64_isqrt(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as u64;
    // fix up f64 imprecision
    while x != 0 && (x as u128) * (x as u128) > v as u128 {
        x -= 1;
    }
    while ((x + 1) as u128) * ((x + 1) as u128) <= v as u128 {
        x += 1;
    }
    x
}

/// `floor(a^(1/k))` for `k >= 1` - Newton iteration.
pub fn iroot(a: &[Limb], k: u64) -> Vec<Limb> {
    assert!(k >= 1, "iroot: k must be >= 1");
    if k == 1 {
        return trimmed(a);
    }
    if k == 2 {
        return isqrt(a);
    }
    let bl = bit_len(a);
    if bl == 0 {
        return vec![];
    }
    if k as u128 >= bl as u128 {
        // 2^k > a  =>  root is 1 (a >= 1 here)
        return vec![1];
    }
    let k = k as usize;
    // x0 = 2^ceil(bl/k)  =>  x0^k >= 2^bl > a
    let mut x = shl(&[1u64], bl.div_ceil(k));
    loop {
        // n / x^(k-1); skip the big pow when the quotient is clearly zero
        let xbits = bit_len(&x);
        let q = if (k - 1) * (xbits - 1) > bl {
            vec![]
        } else {
            let xp = pow(&x, (k - 1) as u64);
            div_rem(a, &xp).0
        };
        let nx = {
            let t = add(&mul_small(&x, (k - 1) as u64), &q);
            div_rem_small(&t, k as u64).0
        };
        if cmp(&nx, &x) != Ordering::Less {
            break;
        }
        x = nx;
    }
    while cmp(&pow(&x, k as u64), a) == Ordering::Greater {
        x = sub(&x, &[1]);
    }
    loop {
        let x1 = add(&x, &[1]);
        if cmp(&pow(&x1, k as u64), a) != Ordering::Greater {
            x = x1;
        } else {
            break;
        }
    }
    x
}

/// `round(sqrt(a))` with ties away from zero: `(isqrt(4a) + 1) >> 1`.
pub fn isqrt_round(a: &[Limb]) -> Vec<Limb> {
    let s4 = isqrt(&shl(a, 2));
    let mut r = add(&s4, &[1]);
    r = shr(&r, 1);
    r
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic xorshift RNG for property tests (no external deps).
    pub struct Rng(u64);
    impl Rng {
        pub fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }
        pub fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        pub fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        /// Random magnitude with `min .. min+extra` limbs.
        pub fn limbs_upto(&mut self, min: usize, extra: u64) -> Vec<Limb> {
            let len = min + self.below(extra) as usize;
            self.limbs(len)
        }
        pub fn limbs(&mut self, len: usize) -> Vec<Limb> {
            let mut v: Vec<Limb> = (0..len).map(|_| self.next()).collect();
            // sprinkle extreme limbs to stress carries
            for w in v.iter_mut() {
                match self.below(5) {
                    0 => *w = u64::MAX,
                    1 => *w = 0,
                    _ => {}
                }
            }
            trim(&mut v);
            v
        }
    }

    fn to_u128(a: &[Limb]) -> u128 {
        let mut r = 0u128;
        for (i, &w) in a.iter().enumerate().take(2) {
            r |= (w as u128) << (64 * i);
        }
        r
    }

    #[test]
    fn add_sub_roundtrip() {
        let mut rng = Rng::new(42);
        for _ in 0..2000 {
            let a = rng.limbs_upto(0, 6);
            let b = rng.limbs_upto(0, 6);
            let s = add(&a, &b);
            let a2 = sub(&s, &b);
            assert_eq!(trimmed(&a), a2);
            let b2 = sub(&s, &a);
            assert_eq!(trimmed(&b), b2);
        }
    }

    #[test]
    fn add_matches_u128() {
        let mut rng = Rng::new(7);
        for _ in 0..2000 {
            let a = rng.next() as u128;
            let b = rng.next() as u128;
            let r = add(&from_u64(a as u64), &from_u64(b as u64));
            assert_eq!(to_u128(&r), a + b);
        }
    }

    #[test]
    fn mul_matches_u128() {
        let mut rng = Rng::new(9);
        for _ in 0..2000 {
            let a = rng.next();
            let b = rng.next();
            let r = mul(&from_u64(a), &from_u64(b));
            assert_eq!(to_u128(&r), a as u128 * b as u128);
        }
    }

    #[test]
    fn karatsuba_matches_schoolbook() {
        let mut rng = Rng::new(1234);
        for &(la, lb) in &[
            (40usize, 40usize),
            (33, 64),
            (100, 97),
            (300, 40),
            (64, 300),
            (65, 65),
            (32, 31),
        ] {
            let a = rng.limbs(la);
            let b = rng.limbs(lb);
            assert_eq!(mul(&a, &b), mul_schoolbook(&a, &b), "size {}x{}", la, lb);
        }
    }

    #[test]
    fn shifts() {
        let mut rng = Rng::new(5);
        for _ in 0..500 {
            let a = rng.limbs_upto(1, 5);
            let k = rng.below(300) as usize;
            let s = shl(&a, k);
            assert_eq!(shr(&s, k), trimmed(&a));
            if !is_zero(&a) {
                assert_eq!(bit_len(&s), bit_len(&a) + k);
            }
        }
    }

    #[test]
    fn round_shr_ties_away() {
        // 0b1011 >> 1: 5.5 -> 6 (away from zero)
        assert_eq!(round_shr(&[0b1011], 1), vec![6]);
        // 0b1010 >> 1: 5.0 -> 5
        assert_eq!(round_shr(&[0b1010], 1), vec![5]);
        // 0b101 >> 2 : 1.25 -> 1
        assert_eq!(round_shr(&[0b101], 2), vec![1]);
        // 0b110 >> 2 : 1.5 -> 2
        assert_eq!(round_shr(&[0b110], 2), vec![2]);
        // 0b111 >> 2 : 1.75 -> 2
        assert_eq!(round_shr(&[0b111], 2), vec![2]);
        // all-ones carry propagation
        assert_eq!(round_shr(&[u64::MAX], 1), vec![1u64 << 63]);
    }

    #[test]
    fn div_rem_properties() {
        let mut rng = Rng::new(4242);
        for it in 0..4000 {
            let u = rng.limbs_upto(1, 12);
            let mut v = rng.limbs_upto(1, 6);
            if is_zero(&v) {
                v = vec![1 + rng.below(1000)];
            }
            let (q, r) = div_rem(&u, &v);
            assert!(cmp(&r, &v) == Ordering::Less, "iter {}: r < v violated", it);
            let back = add(&mul(&q, &v), &r);
            assert_eq!(back, trimmed(&u), "iter {}: q*v+r != u", it);
        }
    }

    #[test]
    fn div_rem_edge_cases() {
        // exact multiples
        let v = vec![u64::MAX, u64::MAX, 1];
        let q0 = vec![0x1234_5678_9abc_def0, u64::MAX, u64::MAX, 7];
        let u = mul(&q0, &v);
        let (q, r) = div_rem(&u, &v);
        assert_eq!(q, q0);
        assert!(is_zero(&r));
        // u < v
        let (q, r) = div_rem(&[5], &[7, 1]);
        assert!(q.is_empty());
        assert_eq!(r, vec![5]);
        // division by 1
        let (q, r) = div_rem(&[1, 2, 3], &[1]);
        assert_eq!(q, vec![1, 2, 3]);
        assert!(is_zero(&r));
        // add-back stress: divisors just below powers of two
        let mut rng = Rng::new(99);
        for _ in 0..2000 {
            let u = vec![rng.next(), rng.next(), rng.next(), rng.next()];
            let v = vec![rng.next(), 1u64 << 63]; // normalized-ish top
            let (q, r) = div_rem(&u, &v);
            assert!(cmp(&r, &v) == Ordering::Less);
            assert_eq!(add(&mul(&q, &v), &r), trimmed(&u));
        }
    }

    #[test]
    fn isqrt_properties() {
        let mut rng = Rng::new(31337);
        for _ in 0..600 {
            let a = rng.limbs_upto(1, 8);
            let s = isqrt(&a);
            assert!(cmp(&mul(&s, &s), &a) != Ordering::Greater);
            let s1 = add(&s, &[1]);
            assert!(cmp(&mul(&s1, &s1), &a) == Ordering::Greater);
        }
        // perfect squares
        for _ in 0..200 {
            let x = rng.limbs_upto(1, 4);
            let sq = mul(&x, &x);
            assert_eq!(isqrt(&sq), trimmed(&x));
        }
    }

    #[test]
    fn iroot_properties() {
        let mut rng = Rng::new(2718);
        for &k in &[1u64, 2, 3, 5, 17, 64] {
            for _ in 0..120 {
                let a = rng.limbs_upto(1, 5);
                if is_zero(&a) {
                    continue;
                }
                let r = iroot(&a, k);
                assert!(cmp(&pow(&r, k), &a) != Ordering::Greater, "k={}", k);
                let r1 = add(&r, &[1]);
                assert!(cmp(&pow(&r1, k), &a) == Ordering::Greater, "k={}", k);
            }
        }
        // huge k
        assert_eq!(iroot(&[u64::MAX, u64::MAX], 100_000), vec![1]);
    }

    #[test]
    fn pow10_works() {
        assert_eq!(pow10(0), vec![1]);
        assert_eq!(pow10(1), vec![10]);
        assert_eq!(pow10(19), vec![10_000_000_000_000_000_000]);
        let p38 = pow10(38);
        assert_eq!(
            to_u128(&p38),
            100_000_000_000_000_000_000_000_000_000_000_000_000u128
        );
        // 10^40 / 10^20 == 10^20
        let (q, r) = div_rem(&pow10(40), &pow10(20));
        assert!(is_zero(&r));
        assert_eq!(q, pow10(20));
    }
}

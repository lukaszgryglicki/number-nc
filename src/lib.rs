//! # NF / NC arbitrary-precision fixed-point arithmetic
//!
//! An `NF` number ("N-Float") with parameter `N` is a fixed-point number with:
//!
//! * `2^N` bits for the part **before** the decimal point,
//! * `2^N` bits for the part **after** the decimal point (the point is always
//!   exactly in the middle),
//! * plus 4 special bits that are always present:
//!   1. **NaN** bit  - the number is invalid (result of an undefined operation),
//!   2. **Inf** bit  - overflow: the true magnitude is larger than the largest
//!      representable value (`+Inf` / `-Inf` together with the sign bit),
//!   3. **Eps** bit  - underflow: the true magnitude is smaller than the
//!      smallest representable positive value but **not** zero
//!      (`+InfSmall` / `-InfSmall` together with the sign bit),
//!   4. **Sign** bit - `0` = positive, `1` = negative.
//!
//! Total logical size: `2*(2^N) + 4` bits (e.g. `0F`=6, `1F`=8, `2F`=12,
//! `3F`=20, `4F`=36, `10F`=2052 bits).  No additional fixed bits are needed:
//! zero is simply an all-zero payload, and every special quantity
//! (`NaN`, `+Inf`, `-Inf`, `+InfSmall`, `-InfSmall`) is expressible with the
//! 4 bits above.
//!
//! ## The `Any` state (`Inf` and `Eps` bits both set)
//!
//! One extra flag combination is meaningful: **both** the Inf and the Eps
//! bit set (with a clear NaN bit) encodes `+Any` / `-Any` - "a value whose
//! sign is known but whose magnitude is completely indeterminate, anywhere
//! in `(0, +Inf]`".  It is produced by indeterminate forms whose *sign* is
//! nevertheless certain, most notably `+Inf * +InfSmall = +Any` (the result
//! can be infinitesimal, any finite number, or infinite - but never zero,
//! never negative).  Also `Inf/Inf`, `InfSmall/InfSmall`, etc.  Indeterminate
//! forms with an *unknown sign* (`Inf - Inf`, `0 * Inf`) stay `NaN`.
//!
//! ## Global precision
//!
//! `N` is a **process-wide configuration value**: it is identical for every
//! number within a single invocation, so it is *not* stored inside numbers
//! and *not* passed to functions.  Set it once with [`nf::set_n`] before
//! doing any arithmetic.
//!
//! ## In-memory representation (chosen for speed)
//!
//! A number is a bare array of 64-bit machine words (`Vec<u64>` /
//! `&[u64]`) - no structs.  Layout (see the [`nf`] module):
//!
//! ```text
//! [ mantissa limb 0 (least significant) , ... , mantissa limb L-1 , FLAGS ]
//!    L = ceil(2^(N+1) / 64) limbs, fixed point: value = mantissa / 2^(2^N)
//!    FLAGS word: bit0 = NaN, bit1 = Inf, bit2 = Eps(InfSmall), bit3 = Sign
//! ```
//!
//! The mantissa is machine-word aligned so all arithmetic runs directly on
//! 64-bit words (fastest possible); the exact logical `2*(2^N)+4`-bit packed
//! form is available via [`nf::to_bits`] / [`nf::from_bits`].
//!
//! `N` is **not artificially limited**: any `N` is accepted until the machine
//! runs out of address space / RAM (the process may be OOM-killed - that is
//! the only limit).
//!
//! An `NC` number ("N-Complex") is two NF numbers back to back:
//! `[ re..., im... ]`, see the [`nc`] module.
//!
//! I/O is string based: [`nf::from_str`] / [`nf::to_string`] parse/print
//! decimal strings as long as the precision allows, and the [`expr`] module
//! implements a full complex-expression calculator.

pub mod expr;
pub mod fx;
pub mod limb;
pub mod nc;
pub mod nf;
pub mod real;

pub use limb::Limb;

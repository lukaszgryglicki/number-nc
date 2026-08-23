//! In-memory confirmation that enormous precisions work end to end.
//!
//! Adds two NC numbers whose data bits (all `2^N` integer and `2^N`
//! fraction bits of both components) are fully random, then spot-checks
//! the exact fixed-point sum without ever printing a digit (at N = 32 one
//! number is 2*(2^32)+4 logical bits, i.e. ~1 GiB per NF component and
//! ~2 GiB per NC value - decimal printing makes no sense there).
//!
//!     cargo run --release --example confirm32 [N]     (default N = 32)

use nf::nf::{self as nfm};
use nf::{nc, Limb};

/// xorshift64* - tiny deterministic-quality PRNG, no dependencies.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// A positive normal NF with every mantissa (data) bit random.
fn random_nf(rng: &mut Rng) -> Vec<Limb> {
    let limbs = nfm::mant_limbs();
    let mut v = Vec::with_capacity(limbs + 1);
    for _ in 0..limbs {
        v.push(rng.next());
    }
    v.push(0); // flags nibble: normal, positive
    v
}

fn state(x: &[Limb]) -> &'static str {
    if nfm::is_nan(x) {
        "NaN"
    } else if nfm::is_inf(x) {
        "Inf"
    } else if nfm::is_eps(x) {
        "Eps"
    } else {
        "normal"
    }
}

fn vm_hwm_gib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let kb: f64 = rest.trim().trim_end_matches(" kB").trim().parse().unwrap_or(0.0);
            return kb / (1024.0 * 1024.0);
        }
    }
    0.0
}

fn main() {
    let n: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(32);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 | 1)
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut rng = Rng(seed);

    nfm::set_n(n);
    let limbs = nfm::mant_limbs();
    println!(
        "N={}  logical bits per NF: 2*(2^{})+4 = {}  ({} limbs, {:.3} GiB/NF, {:.3} GiB/NC)  seed={:#x}",
        n,
        n,
        2 * (1u64 << n) + 4,
        limbs,
        (limbs as f64) * 8.0 / (1024.0 * 1024.0 * 1024.0),
        (limbs as f64) * 16.0 / (1024.0 * 1024.0 * 1024.0),
        seed
    );

    let t0 = std::time::Instant::now();
    let a_re = random_nf(&mut rng);
    let a_im = random_nf(&mut rng);
    let b_re = random_nf(&mut rng);
    let b_im = random_nf(&mut rng);
    // keep the low limbs for the exact spot-check below
    let (lo_a_re, lo_a_im, lo_b_re, lo_b_im) = (a_re[0], a_im[0], b_re[0], b_im[0]);
    println!("filled 4 random NF mantissas in {:.2?}", t0.elapsed());

    let t1 = std::time::Instant::now();
    let a = nc::make(&a_re, &a_im);
    let b = nc::make(&b_re, &b_im);
    drop(a_re);
    drop(a_im);
    drop(b_re);
    drop(b_im);

    let c = nc::add(&a, &b);
    let dt = t1.elapsed();

    let (cre, cim) = (nc::re(&c), nc::im(&c));
    println!("nc::add done in {:.2?}  ->  re: {}, im: {}", dt, state(cre), state(cim));

    // exact fixed-point addition of positive normals: the lowest limb of the
    // sum must be the wrapping sum of the operands' lowest limbs (no borrow
    // can reach it), unless the whole component overflowed to +Inf.
    let mut ok = true;
    for (name, comp, la, lb) in
        [("re", cre, lo_a_re, lo_b_re), ("im", cim, lo_a_im, lo_b_im)]
    {
        if nfm::is_inf(comp) {
            println!("{}: overflowed to +Inf (legitimate: random top bits carried out)", name);
        } else if comp[0] == la.wrapping_add(lb) {
            println!("{}: exact low-limb check passed", name);
        } else {
            println!("{}: LOW-LIMB MISMATCH", name);
            ok = false;
        }
    }

    println!("peak RSS: {:.2} GiB", vm_hwm_gib());
    if ok {
        println!("OK: {}C addition confirmed in memory", n);
    } else {
        println!("FAILED");
        std::process::exit(1);
    }
}

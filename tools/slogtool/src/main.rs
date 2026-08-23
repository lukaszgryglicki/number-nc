//! Fractional tetration reach of NF max values, computed with the
//! claude-tetration crate (Kouznetsov method): reach_b(N) = slog_b(max(N)),
//! inverted from F_b via exact log_b reductions + bisection on h in [0,1].
//! Bases: 1.5, 2, e, 10. F_b is sampled once on a fine grid per base.
use rug::{Complex, Float};
use tetration::cnum::digits_to_bits;
use tetration::kouznetsov::{eval_kouznetsov, setup_kouznetsov, KouznetsovState};
use tetration::regions::{classify, Region};

const DIGITS: u64 = 8;
const GRID: usize = 256; // F_b sampled at h = i/GRID, i = 0..=GRID

fn fpdata(r: Region) -> tetration::regions::FixedPointData {
    match r {
        Region::ShellThronInterior(d)
        | Region::ShellThronBoundary(d)
        | Region::OutsideShellThronRealPositive(d)
        | Region::OutsideShellThronGeneral(d) => d,
        _ => panic!("unexpected region"),
    }
}

struct Base {
    bf: f64,
    grid: Vec<f64>, // F_b(i/GRID), monotone increasing from 1 to b
}

impl Base {
    fn new(bf: f64, exact_e: bool) -> Self {
        let prec = digits_to_bits(DIGITS);
        let b = if exact_e {
            Complex::with_val(prec, Float::with_val(prec, 1u32).exp())
        } else {
            Complex::with_val(prec, Float::with_val(prec, bf))
        };
        let st: KouznetsovState =
            setup_kouznetsov(&b, &fpdata(classify(&b, prec).unwrap()), prec, DIGITS)
                .expect("setup");
        let grid: Vec<f64> = (0..=GRID)
            .map(|i| {
                let h = i as f64 / GRID as f64;
                let hc = Complex::with_val(prec, Float::with_val(prec, h));
                eval_kouznetsov(&st, &b, &hc).expect("eval").real().to_f64()
            })
            .collect();
        for w in grid.windows(2) {
            assert!(w[1] > w[0], "grid not monotone");
        }
        eprintln!("base {bf}: setup+grid done, F(0)={}, F(1)={}", grid[0], grid[GRID]);
        Base { bf, grid }
    }

    /// invert F_b on [0,1] by monotone grid + local linear interpolation
    fn inv_f(&self, y: f64) -> f64 {
        let g = &self.grid;
        let (mut lo, mut hi) = (0usize, GRID);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if g[mid] < y {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t = (y - g[lo]) / (g[hi] - g[lo]);
        (lo as f64 + t) / GRID as f64
    }

    /// slog_b of a positive value given directly (v) or as log2(v) (l2).
    fn slog(&self, v: Option<f64>, l2: Option<f64>) -> f64 {
        let lb = self.bf.ln();
        let (mut y, mut k) = match (v, l2) {
            (Some(v), _) => (v, 0i64),
            (None, Some(l2)) => (l2 * 2f64.ln() / lb, 1i64), // one log_b applied
            _ => unreachable!(),
        };
        while y > self.bf {
            y = y.ln() / lb;
            k += 1;
        }
        while y <= 1.0 {
            // for tiny maxima (N=0 caps below 1? not here) keep in (1,b]
            y = self.bf.powf(y);
            k -= 1;
        }
        k as f64 + self.inv_f(y)
    }
}

/// exact integer reach for base 2: tet(2,h) <= 2^F - 2^-F  iff  tet(2,h-1) <= F-1
fn k2_exact(n: u32) -> u32 {
    let f: u128 = 1u128 << n;
    let (mut h, mut t) = (0u32, 1u128);
    while t <= f - 1 {
        h += 1;
        if t >= 128 {
            return h; // next tower is 2^t >= 2^128 > any F-1 here
        }
        t = 1u128 << t;
    }
    h
}

fn trunc2(x: f64) -> f64 {
    (x * 100.0).floor() / 100.0
}

fn main() {
    let bases = [
        Base::new(1.5, false),
        Base::new(2.0, false),
        Base::new(std::f64::consts::E, true),
        Base::new(10.0, false),
    ];

    // validation against the tet CLI transcript + exact towers
    let v = |x: f64, want: f64| {
        assert!((x - want).abs() < 5e-3, "validation: got {x}, want {want}");
    };
    v(bases[1].slog(Some(5.8248181e31), None), 4.50); // tet 8 2 0 4.5 0
    v(bases[1].slog(Some(1.66747e6), None), 4.10); // tet 6 2 0 4.1 0
    v(bases[1].slog(Some(20.6692), None), 3.10); // tet 6 2 0 3.1 0
    v(bases[1].slog(Some(65536.0), None), 4.00); // 2^^4
    v(bases[2].slog(Some(15.154262241479262), None), 2.00); // e^e
    v(bases[3].slog(Some(1e10), None), 2.00); // 10^10
    v(bases[0].slog(Some(1.28115), None), 0.50); // tet 6 1.5 0 0.5 0
    v(bases[0].slog(Some(1.5), None), 1.00);
    v(bases[0].slog(Some(1.5f64.powf(1.5)), None), 2.00);
    eprintln!("validation OK");

    for n in 0u32..=50 {
        let mut cells = Vec::new();
        for (i, base) in bases.iter().enumerate() {
            let f = 1u128 << n;
            let reach = if n <= 9 {
                let ff = f as f64;
                base.slog(Some((2f64).powf(ff) - (2f64).powf(-ff)), None)
            } else {
                base.slog(None, Some(f as f64))
            };
            let mut r = trunc2(reach);
            if i == 1 {
                // clamp against the exact integer tower rule (near-miss rows)
                let k = k2_exact(n) as f64;
                if r >= k + 1.0 {
                    r = k + 0.99;
                }
                if r < k {
                    r = k;
                }
            }
            cells.push(format!("{:.2}", r));
        }
        println!("| {} | {} | {} | {} | {} |", n, cells[0], cells[1], cells[2], cells[3]);
    }
}

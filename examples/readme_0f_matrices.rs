//! Generates the "complete 0F matrices" tables embedded in README.md.
//!
//! Run with:  cargo run --release --example readme_0f_matrices
//!
//! At N=0 the argument set {NaN, ±Inf, ±Any, ±InfSmall, 0, ±0.5, ±1, ±1.5}
//! covers every representable 0F magnitude, so these 14×14 grids show the
//! whole format under add / mul / div / pow. Every cell is computed by the
//! library (and independently cross-checked bit-exactly by `make test
//! ALL0F=1`, which runs all 64×64 bit patterns per operation).

use nf::nf as nfm;
use nf::real;

type V = Vec<u64>;
type Op = fn(&[u64], &[u64]) -> V;

fn mk(s: &str) -> V {
    match s {
        "X" => nfm::nan(),
        "+∞" => nfm::inf(false),
        "−∞" => nfm::inf(true),
        "+A" => nfm::any(false),
        "−A" => nfm::any(true),
        "+ε" => nfm::eps(false),
        "−ε" => nfm::eps(true),
        "0" => nfm::zero(),
        _ => nfm::from_str(&s.replace('−', "-")).unwrap(),
    }
}

fn short(v: &V) -> String {
    match nfm::to_string_digits(v, None).as_str() {
        "NaN" => "X".into(),
        "+Inf" => "+∞".into(),
        "-Inf" => "−∞".into(),
        "+Any" => "+A".into(),
        "-Any" => "−A".into(),
        "+InfSmall" => "+ε".into(),
        "-InfSmall" => "−ε".into(),
        s => s.replace('-', "−"),
    }
}

fn main() {
    nfm::set_n(0);
    let labels = [
        "X", "+∞", "−∞", "+A", "−A", "+ε", "−ε", "0", "0.5", "−0.5", "1",
        "−1", "1.5", "−1.5",
    ];
    let vals: Vec<V> = labels.iter().map(|l| mk(l)).collect();
    let ops: [(&str, Op); 4] = [
        ("Addition (`a + b`)", nfm::add),
        ("Multiplication (`a * b`)", nfm::mul),
        ("Division (`a / b`)", nfm::div),
        ("Power (`a ^ b`)", real::pow),
    ];
    for (name, f) in ops {
        println!("### {name} — complete 0F grid\n");
        print!("| a\\b |");
        for l in labels {
            print!(" {l} |");
        }
        println!();
        print!("|-----|");
        for _ in labels {
            print!("----|");
        }
        println!();
        for (rl, rv) in labels.iter().zip(&vals) {
            print!("| **{rl}** |");
            for cv in &vals {
                print!(" {} |", short(&f(rv, cv)));
            }
            println!();
        }
        println!();
    }
}

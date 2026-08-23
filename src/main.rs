//! `nc` - arbitrary-precision NF/NC calculator.
//!
//! Usage: `nc [options] N "expression"`
//!
//! `N` selects the NF precision: every number in the invocation uses
//! `2^N` integer bits, `2^N` fraction bits and 4 flag bits
//! (NaN, Inf, InfSmall, Sign) - `2*2^N+4` logical bits in total.
//! There is no upper limit on `N` other than available memory.

use nf::{expr, nc, nf as nfmod};
use std::process::ExitCode;

const HELP: &str = "\
nc - arbitrary-precision complex calculator on the NF number format

USAGE:
    nc [OPTIONS] N \"EXPRESSION\"

ARGS:
    N             precision parameter (e.g. 7 or 7F): numbers get 2^N integer
                  bits, 2^N fraction bits and 4 flag bits (NaN/Inf/InfSmall/
                  Sign). Memory is the only upper limit on N.
    EXPRESSION    calculator expression evaluated over NC (complex) numbers

OPTIONS:
    -d, --digits D   print D fraction digits (default: shortest string that
                     parses back to the exact value)
    -b, --bits       also print each component as raw bits:
                     flags(NaN,Inf,InfSmall,Sign)-integer-fraction
    -B, --from-bits  treat EXPRESSION as a raw bit string
                     (e.g. 0001-00000010-00000000; separators optional)
                     and print its decimal value plus canonical bits
    -q, --quiet      compute but print nothing (in-memory confirmation for
                     enormous N where decimal printing is impractical);
                     exit status still reports success/failure
    -h, --help       show this help
    -V, --version    show version

OPERATORS:
    + - * / ^ (or **), parentheses, implicit multiplication: 2pi, (1+2i)(3-i)

CONSTANTS:
    pi tau e i inf eps (infsmall) any nan

FUNCTIONS:
    re im abs arg conj sqrt cbrt root(z,k) exp ln log log2 log10 logb(b,x)
    pow(a,b) atan2(y,x) sin cos tan asin acos atan sinh cosh tanh
    asinh acosh atanh floor ceil round trunc

EXAMPLES:
    nc 7 \"(3.14159+2.73i)^(-2+0.5i)\"
    nc 10 \"exp(i*pi)\"
    nc 4 --bits \"-2\"
    nc 3 --from-bits \"0001-00000010-00000000\"
    nc 8 -d 50 \"sqrt(2)\"

EXIT STATUS:
    0  result computed (including NaN/Inf results)
    1  usage error
    2  expression error (syntax or unknown name)
";

struct Opts {
    n: u32,
    expr: String,
    digits: Option<usize>,
    bits: bool,
    from_bits: bool,
    quiet: bool,
}

fn parse_n(s: &str) -> Option<u32> {
    let t = s.strip_suffix('F').or_else(|| s.strip_suffix('f')).unwrap_or(s);
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<u32>().ok()
}

fn parse_args() -> Result<Option<Opts>, String> {
    let mut n: Option<u32> = None;
    let mut expr_s: Option<String> = None;
    let mut digits: Option<usize> = None;
    let mut bits = false;
    let mut from_bits = false;
    let mut quiet = false;
    let mut no_more_opts = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if no_more_opts {
            if n.is_none() {
                n = Some(parse_n(&a).ok_or_else(|| {
                    format!("N must be a non-negative integer (e.g. 7 or 7F), got '{}'", a)
                })?);
            } else if expr_s.is_none() {
                expr_s = Some(a);
            } else {
                return Err(format!("unexpected extra argument '{}'", a));
            }
            continue;
        }
        match a.as_str() {
            "--" => no_more_opts = true,
            "-h" | "--help" => return Ok(None),
            "-V" | "--version" => {
                println!("nc (NF calculator) {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-b" | "--bits" => bits = true,
            "-B" | "--from-bits" => from_bits = true,
            "-q" | "--quiet" => quiet = true,
            "-d" | "--digits" => {
                let v = args.next().ok_or("--digits needs a value")?;
                digits = Some(v.parse::<usize>().map_err(|_| "--digits: not a number")?);
            }
            _ if a.starts_with("--digits=") => {
                digits = Some(
                    a["--digits=".len()..].parse::<usize>().map_err(|_| "--digits: not a number")?,
                );
            }
            _ if a.starts_with('-') && a.len() > 1 && n.is_some() && expr_s.is_none() => {
                // allow expressions starting with '-' once N is known
                expr_s = Some(a);
            }
            _ if a.starts_with('-') && a.len() > 1 && parse_n(&a).is_none() && expr_s.is_some() => {
                return Err(format!("unknown option '{}'", a));
            }
            _ => {
                if n.is_none() {
                    n = Some(parse_n(&a).ok_or_else(|| {
                        format!("N must be a non-negative integer (e.g. 7 or 7F), got '{}'", a)
                    })?);
                } else if expr_s.is_none() {
                    expr_s = Some(a);
                } else {
                    return Err(format!("unexpected extra argument '{}'", a));
                }
            }
        }
    }
    let n = n.ok_or("missing N argument")?;
    let expr_s = expr_s.ok_or("missing expression argument")?;
    Ok(Some(Opts { n, expr: expr_s, digits, bits, from_bits, quiet }))
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(Some(o)) => o,
        Ok(None) => {
            print!("{}", HELP);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("nc: {}", e);
            eprintln!("try 'nc --help'");
            return ExitCode::from(1);
        }
    };

    nfmod::set_n(opts.n);

    if opts.from_bits {
        // raw bit-string mode: decode, print value and canonical bits
        return match nfmod::from_bit_string(&opts.expr) {
            Ok(v) => {
                if !opts.quiet {
                    println!("{}", nfmod::to_string_digits(&v, opts.digits));
                    println!("bits: {}", nfmod::to_bit_string(&v));
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("nc: bad bit string: {}", e);
                ExitCode::from(2)
            }
        };
    }

    let val = match expr::eval_str(&opts.expr) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("nc: error {}", e);
            return ExitCode::from(2);
        }
    };

    if opts.quiet {
        // computed successfully; deliberately print nothing
        return ExitCode::SUCCESS;
    }
    println!("{}", nc::to_string_digits(&val, opts.digits));
    if opts.bits {
        let (re, im) = (nc::re(&val), nc::im(&val));
        println!("re bits: {}", nfmod::to_bit_string(re));
        if !nfmod::is_zero(im) {
            println!("im bits: {}", nfmod::to_bit_string(im));
        }
    }
    ExitCode::SUCCESS
}

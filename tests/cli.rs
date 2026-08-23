//! End-to-end integration tests that run the built `nc` binary.

use std::process::Command;

fn run(args: &[&str]) -> (String, String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_nc"))
        .args(args)
        .output()
        .expect("failed to run nc binary");
    (
        String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn user_example_expression() {
    // the exact invocation from the task statement
    let (out, _, code) = run(&[
        "7",
        "(3.14159+2.73i)^(-2.00000000000000000000000000000000000000000000000000051-0.0000000000000000000000000000000001i)",
    ]);
    assert_eq!(code, 0);
    // double-precision reference: 0.008053784635898781 - 0.057163870131559186i
    assert!(out.starts_with("0.00805378463589878"), "got {}", out);
    assert!(out.contains("-0.0571638701315591"), "got {}", out);
    assert!(out.ends_with('i'), "got {}", out);
}

#[test]
fn cross_precision_agreement() {
    // leading digits must agree between N=7 and N=8
    let expr = "(3.14159+2.73i)^(-2.00000000000000000000000000000000000000000000000000051-0.0000000000000000000000000000000001i)";
    let (o7, _, c7) = run(&["7", "-d", "30", expr]);
    let (o8, _, c8) = run(&["8", "-d", "30", expr]);
    assert_eq!(c7, 0);
    assert_eq!(c8, 0);
    assert_eq!(o7, o8);
}

#[test]
fn n_suffix_accepted() {
    let (a, _, _) = run(&["7", "2^100"]);
    let (b, _, _) = run(&["7F", "2^100"]);
    assert_eq!(a, b);
    assert_eq!(a, "1267650600228229401496703205376");
}

#[test]
fn bits_output_matches_spec_example() {
    // the spec example: N=3, value -2 -> 0001-00000010-00000000
    let (out, _, code) = run(&["3", "--bits", "-2"]);
    assert_eq!(code, 0);
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("-2"));
    assert_eq!(lines.next(), Some("re bits: 0001-00000010-00000000"));
}

#[test]
fn digits_flag() {
    let (out, _, code) = run(&["8", "--digits", "50", "sqrt(2)"]);
    assert_eq!(code, 0);
    assert_eq!(out, "1.41421356237309504880168872420969807856967187537695");
}

#[test]
fn special_results() {
    assert_eq!(run(&["7", "1/0"]).0, "(NaN)");
    assert_eq!(run(&["7", "1/inf"]).0, "(+InfSmall)");
    assert_eq!(run(&["7", "-1/eps"]).0, "(-Inf)");
    assert_eq!(run(&["7", "inf-inf"]).0, "(NaN)");
    assert_eq!(run(&["4", "1000000"]).0, "(+Inf)"); // overflow at N=4
    assert_eq!(run(&["4", "2^-100"]).0, "(+InfSmall)"); // underflow at N=4
    // math errors still exit 0 - they are results, not failures
    assert_eq!(run(&["7", "1/0"]).2, 0);
}

#[test]
fn euler_identity_high_precision() {
    let (out, _, code) = run(&["10", "exp(i*pi)"]);
    assert_eq!(code, 0);
    assert_eq!(out, "-1-(+InfSmall)i");
}

#[test]
fn tiny_and_large_n() {
    // N=0: 1 integer bit, 1 fraction bit
    assert_eq!(run(&["0", "0.5+0.5"]).0, "1");
    assert_eq!(run(&["0", "2"]).0, "(+Inf)");
    // N=12: 4096-bit fractions work fine
    let (out, _, code) = run(&["12", "sqrt(2)^2"]);
    assert_eq!(code, 0);
    assert_eq!(out, "2");
}

#[test]
fn error_exit_codes() {
    // usage errors -> 1
    assert_eq!(run(&[]).2, 1);
    assert_eq!(run(&["notanumber", "1+1"]).2, 1);
    assert_eq!(run(&["7"]).2, 1);
    // expression errors -> 2
    assert_eq!(run(&["7", "2+"]).2, 2);
    assert_eq!(run(&["7", "bogus(1)"]).2, 2);
    assert_eq!(run(&["7", "(2"]).2, 2);
    // help -> 0
    let (h, _, hc) = run(&["--help"]);
    assert_eq!(hc, 0);
    assert!(h.contains("USAGE"));
}

#[test]
fn from_bits_mode() {
    // N=0: the 6-bit extreme; "ffff-i-f"
    assert_eq!(run(&["0", "--from-bits", "0000-0-0"]).0, "0\nbits: 0000-0-0");
    assert_eq!(run(&["0", "-B", "0000-1-1"]).0, "1.5\nbits: 0000-1-1");
    // N=1: the user's 0000-00-00 notation
    assert_eq!(run(&["1", "--from-bits", "0000-00-00"]).0, "0\nbits: 0000-00-00");
    assert_eq!(run(&["1", "--from-bits", "0001-10-00"]).0, "-2\nbits: 0001-10-00");
    // N=3: the specification example (20 bits, -2.0)
    assert_eq!(
        run(&["3", "--from-bits", "0001-00000010-00000000"]).0,
        "-2\nbits: 0001-00000010-00000000"
    );
    // specials decode and canonicalize (payload of a special is cleared)
    assert_eq!(run(&["0", "-B", "0100-1-1"]).0, "+Inf\nbits: 0100-0-0");
    assert_eq!(run(&["0", "-B", "0110-0-0"]).0, "+Any\nbits: 0110-0-0");
    assert_eq!(run(&["0", "-B", "0111-0-0"]).0, "-Any\nbits: 0111-0-0");
    assert_eq!(run(&["0", "-B", "1010-0-0"]).0, "NaN\nbits: 1000-0-0");
    // -0 canonicalizes to +0
    assert_eq!(run(&["0", "-B", "0001-0-0"]).0, "0\nbits: 0000-0-0");
    // separators are optional
    assert_eq!(run(&["0", "-B", "000011"]).0, "1.5\nbits: 0000-1-1");
    // malformed strings -> exit 2
    assert_eq!(run(&["0", "-B", "00"]).2, 2);
    assert_eq!(run(&["0", "-B", "0000-2-0"]).2, 2);
}

#[test]
fn any_state_cli() {
    assert_eq!(run(&["3", "inf*eps"]).0, "(+Any)");
    assert_eq!(run(&["3", "-inf*eps"]).0, "(-Any)");
    assert_eq!(run(&["3", "inf/inf"]).0, "(+Any)");
    assert_eq!(run(&["3", "eps/eps"]).0, "(+Any)");
    assert_eq!(run(&["3", "any"]).0, "(+Any)");
    assert_eq!(run(&["3", "any-any"]).0, "(NaN)");
    // bit pattern of Any has both the Inf and the InfSmall bits set
    let (out, _, code) = run(&["3", "--bits", "inf*eps"]);
    assert_eq!(code, 0);
    assert_eq!(out, "(+Any)\nre bits: 0110-00000000-00000000");
}

#[test]
fn huge_n24_cli() {
    // N=24: 33_554_436 logical bits (~4 MB per number), end to end
    assert_eq!(
        run(&["24", "2^100"]).0,
        "1267650600228229401496703205376"
    );
    assert_eq!(run(&["24", "-d", "10", "1/3"]).0, "0.3333333333");
    assert_eq!(run(&["24", "(2+3i)*(2-3i)"]).0, "13");
    assert_eq!(run(&["24", "inf*eps"]).0, "(+Any)");
    assert_eq!(run(&["24", "2^-100 * 2^100"]).0, "1");
}

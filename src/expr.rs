//! Calculator expression parser and evaluator over NC numbers.
//!
//! Grammar (precedence low to high):
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := factor (('*' | '/') factor | implicit-mul factor)*
//! factor  := ('+' | '-')* power
//! power   := atom ('^' factor)?              // right-associative
//! atom    := number | constant | func '(' expr (',' expr)* ')' | '(' expr ')'
//! ```
//!
//! Implicit multiplication is allowed when a value is directly followed by
//! `(`, an identifier, or a number: `2pi`, `(1+2i)(3-i)`, `3i`.
//!
//! Number literals may carry an `i` suffix (`2.5i`), use scientific notation
//! (`1.5e-3`), and `_` separators.  Constants: `pi`, `e`, `i`, `inf`, `eps`
//! (aka `infsmall`), `nan`.  `**` is accepted as a synonym for `^`.

use crate::limb::Limb;
use crate::nc;
use crate::nf;
use crate::real;

/// Parse-or-eval error with a byte position in the input.
#[derive(Debug, Clone)]
pub struct Error {
    pub pos: usize,
    pub msg: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at position {}: {}", self.pos + 1, self.msg)
    }
}

fn err<T>(pos: usize, msg: impl Into<String>) -> Result<T, Error> {
    Err(Error { pos, msg: msg.into() })
}

// ---------------------------------------------------------------------------
// tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// number literal (digits kept as text; converted at eval time), and
    /// whether it carried an `i` suffix
    Num(String, bool),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
}

struct Lexer {
    toks: Vec<(Tok, usize)>,
}

fn lex(src: &str) -> Result<Lexer, Error> {
    let b = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '+' => {
                toks.push((Tok::Plus, i));
                i += 1;
            }
            '-' => {
                toks.push((Tok::Minus, i));
                i += 1;
            }
            '*' => {
                if i + 1 < b.len() && b[i + 1] == b'*' {
                    toks.push((Tok::Caret, i));
                    i += 2;
                } else {
                    toks.push((Tok::Star, i));
                    i += 1;
                }
            }
            '/' => {
                toks.push((Tok::Slash, i));
                i += 1;
            }
            '^' => {
                toks.push((Tok::Caret, i));
                i += 1;
            }
            '(' => {
                toks.push((Tok::LParen, i));
                i += 1;
            }
            ')' => {
                toks.push((Tok::RParen, i));
                i += 1;
            }
            ',' => {
                toks.push((Tok::Comma, i));
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                let mut seen_dot = false;
                let mut txt = String::new();
                while i < b.len() {
                    let d = b[i] as char;
                    if d.is_ascii_digit() || d == '_' {
                        txt.push(d);
                        i += 1;
                    } else if d == '.' && !seen_dot {
                        seen_dot = true;
                        txt.push(d);
                        i += 1;
                    } else {
                        break;
                    }
                }
                if txt.chars().all(|ch| ch == '.' || ch == '_') {
                    return err(start, "invalid number");
                }
                // exponent part: e/E followed by optional sign and digits.
                // `e` not followed by a digit/sign+digit is the constant e.
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && (b[j] as char).is_ascii_digit() {
                        txt.push('e');
                        if b[i + 1] == b'+' || b[i + 1] == b'-' {
                            txt.push(b[i + 1] as char);
                        }
                        i = j;
                        while i < b.len() && (b[i] as char).is_ascii_digit() {
                            txt.push(b[i] as char);
                            i += 1;
                        }
                    }
                }
                // imaginary suffix: `i` not starting a longer identifier
                let mut imag = false;
                if i < b.len() && (b[i] == b'i' || b[i] == b'I') {
                    let next_alnum = i + 1 < b.len()
                        && ((b[i + 1] as char).is_ascii_alphanumeric() || b[i + 1] == b'_');
                    if !next_alnum {
                        imag = true;
                        i += 1;
                    }
                }
                toks.push((Tok::Num(txt, imag), start));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let start = i;
                let mut id = String::new();
                while i < b.len() {
                    let d = b[i] as char;
                    if d.is_ascii_alphanumeric() || d == '_' {
                        id.push(d.to_ascii_lowercase());
                        i += 1;
                    } else {
                        break;
                    }
                }
                toks.push((Tok::Ident(id), start));
            }
            _ => return err(i, format!("unexpected character '{}'", c)),
        }
    }
    Ok(Lexer { toks })
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Ast {
    Num(String, bool),
    Const(String, usize),
    Call(String, usize, Vec<Ast>),
    Neg(Box<Ast>),
    Add(Box<Ast>, Box<Ast>),
    Sub(Box<Ast>, Box<Ast>),
    Mul(Box<Ast>, Box<Ast>),
    Div(Box<Ast>, Box<Ast>),
    Pow(Box<Ast>, Box<Ast>),
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }
    fn here(&self) -> usize {
        self.toks.get(self.pos).map(|(_, p)| *p).unwrap_or(self.end)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|(t, _)| t.clone());
        self.pos += 1;
        t
    }
    fn expect(&mut self, want: &Tok, what: &str) -> Result<(), Error> {
        if self.peek() == Some(want) {
            self.pos += 1;
            Ok(())
        } else {
            err(self.here(), format!("expected {}", what))
        }
    }

    fn expr(&mut self) -> Result<Ast, Error> {
        let mut lhs = self.term()?;
        loop {
            match self.peek() {
                Some(Tok::Plus) => {
                    self.pos += 1;
                    let rhs = self.term()?;
                    lhs = Ast::Add(Box::new(lhs), Box::new(rhs));
                }
                Some(Tok::Minus) => {
                    self.pos += 1;
                    let rhs = self.term()?;
                    lhs = Ast::Sub(Box::new(lhs), Box::new(rhs));
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn term(&mut self) -> Result<Ast, Error> {
        let mut lhs = self.factor()?;
        loop {
            match self.peek() {
                Some(Tok::Star) => {
                    self.pos += 1;
                    let rhs = self.factor()?;
                    lhs = Ast::Mul(Box::new(lhs), Box::new(rhs));
                }
                Some(Tok::Slash) => {
                    self.pos += 1;
                    let rhs = self.factor()?;
                    lhs = Ast::Div(Box::new(lhs), Box::new(rhs));
                }
                // implicit multiplication: value directly followed by an
                // atom-starter (never by +/-, so `2 -3` stays a subtraction)
                Some(Tok::LParen) | Some(Tok::Ident(_)) | Some(Tok::Num(..)) => {
                    let rhs = self.unary_free_power()?;
                    lhs = Ast::Mul(Box::new(lhs), Box::new(rhs));
                }
                _ => return Ok(lhs),
            }
        }
    }

    /// factor := ('+' | '-')* power
    fn factor(&mut self) -> Result<Ast, Error> {
        match self.peek() {
            Some(Tok::Plus) => {
                self.pos += 1;
                self.factor()
            }
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(Ast::Neg(Box::new(self.factor()?)))
            }
            _ => self.power(),
        }
    }

    /// like factor but with no leading sign (used for implicit mul RHS)
    fn unary_free_power(&mut self) -> Result<Ast, Error> {
        self.power()
    }

    /// power := atom ('^' factor)?   (right-assoc; -2^2 = -(2^2))
    fn power(&mut self) -> Result<Ast, Error> {
        let base = self.atom()?;
        if self.peek() == Some(&Tok::Caret) {
            self.pos += 1;
            let expo = self.factor()?;
            return Ok(Ast::Pow(Box::new(base), Box::new(expo)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Ast, Error> {
        let pos = self.here();
        match self.bump() {
            Some(Tok::Num(txt, imag)) => Ok(Ast::Num(txt, imag)),
            Some(Tok::Ident(id)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = vec![self.expr()?];
                    while self.peek() == Some(&Tok::Comma) {
                        self.pos += 1;
                        args.push(self.expr()?);
                    }
                    self.expect(&Tok::RParen, "')'")?;
                    Ok(Ast::Call(id, pos, args))
                } else {
                    Ok(Ast::Const(id, pos))
                }
            }
            Some(Tok::LParen) => {
                let e = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            Some(t) => err(pos, format!("unexpected token {:?}", t)),
            None => err(pos, "unexpected end of expression"),
        }
    }
}

// ---------------------------------------------------------------------------
// evaluation
// ---------------------------------------------------------------------------

fn eval_const(name: &str, pos: usize) -> Result<Vec<Limb>, Error> {
    Ok(match name {
        "i" | "j" => nc::i_unit(),
        "pi" => nc::from_re(&real::pi()),
        "tau" => nc::from_re(&nf::mul(&real::pi(), &nf::from_u64(2))),
        "e" => nc::from_re(&real::e_const()),
        "inf" | "infinity" => nc::from_re(&nf::inf(false)),
        "eps" | "infsmall" | "epsilon" => nc::from_re(&nf::eps(false)),
        "any" => nc::from_re(&nf::any(false)),
        "nan" => nc::from_re(&nf::nan()),
        _ => return err(pos, format!("unknown constant '{}'", name)),
    })
}

fn eval_call(name: &str, pos: usize, args: &[Vec<Limb>]) -> Result<Vec<Limb>, Error> {
    let need = |k: usize| -> Result<(), Error> {
        if args.len() == k {
            Ok(())
        } else {
            err(
                pos,
                format!("{}() takes {} argument{}", name, k, if k == 1 { "" } else { "s" }),
            )
        }
    };
    let one_arg = |f: &dyn Fn(&[Limb]) -> Vec<Limb>| -> Result<Vec<Limb>, Error> {
        need(1)?;
        Ok(f(&args[0]))
    };
    // functions with a real-valued NF result
    let re_of = |v: Vec<Limb>| nc::from_re(&v);
    match name {
        "re" | "real" => {
            need(1)?;
            Ok(re_of(nc::re(&args[0]).to_vec()))
        }
        "im" | "imag" => {
            need(1)?;
            Ok(re_of(nc::im(&args[0]).to_vec()))
        }
        "abs" | "mod" => {
            need(1)?;
            Ok(re_of(nc::abs(&args[0])))
        }
        "arg" | "phase" => {
            need(1)?;
            Ok(re_of(nc::arg(&args[0])))
        }
        "conj" => one_arg(&|z| nc::conj(z)),
        "sqrt" => one_arg(&|z| nc::sqrt(z)),
        "cbrt" => one_arg(&|z| nc::root(z, 3)),
        "exp" => one_arg(&|z| nc::exp(z)),
        "ln" | "log" => one_arg(&|z| nc::ln(z)),
        "log2" => {
            need(1)?;
            Ok(logb_impl(&nc::from_re(&nf::from_u64(2)), &args[0]))
        }
        "log10" => {
            need(1)?;
            Ok(logb_impl(&nc::from_re(&nf::from_u64(10)), &args[0]))
        }
        "logb" => {
            need(2)?;
            Ok(logb_impl(&args[0], &args[1]))
        }
        "pow" => {
            need(2)?;
            Ok(nc::pow(&args[0], &args[1]))
        }
        "root" => {
            need(2)?;
            // exact k-th root for positive integer k; any other exponent
            // (special values, non-integers, complex) via a^(1/b)
            if let Some(k) = real_small_uint(&args[1]) {
                if k > 0 {
                    return Ok(nc::root(&args[0], k));
                }
            }
            Ok(nc::pow(&args[0], &nc::div(&nc::one(), &args[1])))
        }
        "atan2" => {
            need(2)?;
            if !nf::is_zero(nc::im(&args[0])) || !nf::is_zero(nc::im(&args[1])) {
                return err(pos, "atan2() takes real arguments");
            }
            Ok(re_of(real::atan2(nc::re(&args[0]), nc::re(&args[1]))))
        }
        "sin" => one_arg(&|z| nc::sin(z)),
        "cos" => one_arg(&|z| nc::cos(z)),
        "tan" => one_arg(&|z| nc::tan(z)),
        "asin" | "arcsin" => one_arg(&|z| nc::asin(z)),
        "acos" | "arccos" => one_arg(&|z| nc::acos(z)),
        "atan" | "arctan" => one_arg(&|z| nc::atan(z)),
        "sinh" => one_arg(&|z| nc::sinh(z)),
        "cosh" => one_arg(&|z| nc::cosh(z)),
        "tanh" => one_arg(&|z| nc::tanh(z)),
        "asinh" | "arcsinh" => one_arg(&|z| nc::asinh(z)),
        "acosh" | "arccosh" => one_arg(&|z| nc::acosh(z)),
        "atanh" | "arctanh" => one_arg(&|z| nc::atanh(z)),
        "floor" => one_arg(&|z| nc::floor(z)),
        "ceil" => one_arg(&|z| nc::ceil(z)),
        "round" => one_arg(&|z| nc::round(z)),
        "trunc" => one_arg(&|z| nc::trunc(z)),
        _ => err(pos, format!("unknown function '{}'", name)),
    }
}

/// log base b: ln(x)/ln(b) over NC.
fn logb_impl(b: &[Limb], x: &[Limb]) -> Vec<Limb> {
    nc::div(&nc::ln(x), &nc::ln(b))
}

/// Extract a small positive integer from a purely real NC value.
fn real_small_uint(z: &[Limb]) -> Option<u64> {
    let r = nc::re(z);
    if !nf::is_zero(nc::im(z)) || !nf::is_normal(r) || nf::is_neg(r) || !nf::is_integral(r) {
        return None;
    }
    let int = crate::limb::shr(nf::mant(r), nf::frac_bits());
    if crate::limb::bit_len(&int) > 62 {
        return None;
    }
    Some(*int.first().unwrap_or(&0))
}

fn eval(ast: &Ast) -> Result<Vec<Limb>, Error> {
    Ok(match ast {
        Ast::Num(txt, imag) => {
            let v = nf::from_str(txt)
                .map_err(|e| Error { pos: 0, msg: format!("bad number '{}': {}", txt, e) })?;
            if *imag {
                nc::make(&nf::zero(), &v)
            } else {
                nc::from_re(&v)
            }
        }
        Ast::Const(name, pos) => eval_const(name, *pos)?,
        Ast::Call(name, pos, args) => {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(eval(a)?);
            }
            eval_call(name, *pos, &vals)?
        }
        Ast::Neg(a) => nc::neg(&eval(a)?),
        Ast::Add(a, b) => nc::add(&eval(a)?, &eval(b)?),
        Ast::Sub(a, b) => nc::sub(&eval(a)?, &eval(b)?),
        Ast::Mul(a, b) => nc::mul(&eval(a)?, &eval(b)?),
        Ast::Div(a, b) => nc::div(&eval(a)?, &eval(b)?),
        Ast::Pow(a, b) => nc::pow(&eval(a)?, &eval(b)?),
    })
}

/// Parse and evaluate `src`, returning an NC number (global `N` applies).
pub fn eval_str(src: &str) -> Result<Vec<Limb>, Error> {
    let lx = lex(src)?;
    let mut p = Parser { toks: lx.toks, pos: 0, end: src.len() };
    if p.peek().is_none() {
        return err(0, "empty expression");
    }
    let ast = p.expr()?;
    if p.pos != p.toks.len() {
        return err(p.here(), "unexpected trailing input");
    }
    eval(&ast)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nf::with_n;
    use std::cmp::Ordering;

    fn ev(s: &str) -> Vec<Limb> {
        eval_str(s).unwrap()
    }
    fn evs(s: &str) -> String {
        nc::to_string(&ev(s))
    }

    fn close_str(s: &str, want: &str, k: u64) -> bool {
        let got = ev(s);
        let want = ev(want);
        let d = nc::sub(&got, &want);
        let bound = nf::mul(&nf::min_pos(), &nf::from_u64(k));
        let ok = |x: &[Limb]| {
            let a = nf::abs(x);
            nf::is_zero(&a) || nf::is_eps(&a) || nf::cmp(&a, &bound) != Some(Ordering::Greater)
        };
        ok(nc::re(&d)) && ok(nc::im(&d))
    }

    #[test]
    fn precedence_and_syntax() {
        with_n(7, || {
            assert_eq!(evs("2+3*4"), "14");
            assert_eq!(evs("(2+3)*4"), "20");
            assert_eq!(evs("2^3^2"), "512"); // right-assoc
            assert_eq!(evs("-2^2"), "-4"); // unary binds looser than ^
            assert_eq!(evs("2^-2"), "0.25");
            assert_eq!(evs("2**10"), "1024");
            assert_eq!(evs("7/4"), "1.75");
            assert_eq!(evs("10-4-3"), "3"); // left-assoc
            assert_eq!(evs("1.5e2"), "150");
            assert_eq!(evs("1_000.5"), "1000.5");
            assert_eq!(evs("--3"), "3");
            assert_eq!(evs("2--3"), "5");
        });
    }

    #[test]
    fn complex_literals_and_implicit_mul() {
        with_n(7, || {
            assert_eq!(evs("3i"), "3i");
            assert_eq!(evs("2.5i"), "2.5i");
            assert_eq!(evs("1+2i"), "1+2i");
            assert_eq!(evs("(1+2i)(3-i)"), "5+5i");
            assert_eq!(evs("i*i"), "-1");
            assert_eq!(evs("i^2"), "-1");
            assert_eq!(evs("2(3+4)"), "14");
            assert_eq!(evs("(1+i)(1-i)"), "2");
            assert!(close_str(
                "2pi",
                "6.2831853071795864769252867665590057683943387987502116419",
                2
            ));
            assert_eq!(evs("abs(3+4i)"), "5");
            assert_eq!(evs("re(3+4i)"), "3");
            assert_eq!(evs("im(3+4i)"), "4");
            assert_eq!(evs("conj(3+4i)"), "3-4i");
        });
    }

    #[test]
    fn functions_and_constants() {
        with_n(7, || {
            assert!(close_str("exp(ln(7))", "7", 4));
            assert!(close_str("sqrt(-1)", "i", 0));
            assert!(close_str("e^(i pi)", "-1", 2));
            assert!(close_str("ln(e)", "1", 2));
            assert!(close_str("log10(1000)", "3", 4));
            assert!(close_str("log2(4096)", "12", 4));
            assert!(close_str("logb(3, 81)", "4", 4));
            assert!(close_str("root(27, 3)", "3", 2));
            assert!(close_str("cbrt(-8)", "-2", 2));
            assert!(close_str("pow(1+i, 2)", "2i", 0));
            assert!(close_str("sin(pi/6)", "0.5", 2));
            assert!(close_str("atan2(1, 1)", "pi/4", 2));
            assert_eq!(evs("floor(2.7-1.2i)"), "2-2i");
            assert_eq!(evs("nan"), "(NaN)");
            assert_eq!(evs("1/0"), "(NaN)");
            assert_eq!(evs("inf"), "(+Inf)");
            assert_eq!(evs("-inf"), "(-Inf)");
            assert_eq!(evs("eps"), "(+InfSmall)");
            assert_eq!(evs("1/inf"), "(+InfSmall)");
            assert_eq!(evs("1/eps"), "(+Inf)");
        });
    }

    #[test]
    fn error_reporting() {
        with_n(7, || {
            assert!(eval_str("").is_err());
            assert!(eval_str("2+").is_err());
            assert!(eval_str("(2").is_err());
            assert!(eval_str("2)").is_err());
            assert!(eval_str("foo(1)").is_err());
            assert!(eval_str("bogus").is_err());
            assert!(eval_str("sin()").is_err());
            assert!(eval_str("sin(1,2)").is_err());
            assert!(eval_str("2 @ 3").is_err());
            // root with a non-integer k now falls back to a^(1/k)
            assert!(eval_str("root(2, i)").is_ok());
            assert!(eval_str("root(2, 0.5)").is_ok());
            let e = eval_str("2+*3").unwrap_err();
            assert!(e.pos > 0);
        });
    }

    #[test]
    fn user_example_expression() {
        // the exact expression from the task statement
        let src = "(3.14159+2.73i)^(-2.00000000000000000000000000000000000000000000000000051-0.0000000000000000000000000000000001i)";
        for &n in &[6u32, 7, 8] {
            with_n(n, || {
                let v = eval_str(src).unwrap();
                assert!(nc::is_all_normal(&v), "n={}", n);
                // |z^w| must be < 1 since |z|>1 and re(w)<0
                let m = nc::abs(&v);
                assert!(nf::cmp(&m, &nf::one()) == Some(Ordering::Less), "n={}", n);
                assert!(!nf::is_zero(&m), "n={}", n);
            });
        }
    }

    #[test]
    fn cross_n_consistency() {
        // same expression at N=7 and N=8 must agree on leading digits
        let src = "exp(sin(1.25+0.5i) * ln(2.5-0.75i))";
        let s7 = with_n(7, || nc::to_string_digits(&eval_str(src).unwrap(), Some(30)));
        let s8 = with_n(8, || nc::to_string_digits(&eval_str(src).unwrap(), Some(30)));
        assert_eq!(s7, s8);
    }
}

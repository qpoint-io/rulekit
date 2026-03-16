mod ast;
mod compare;
mod errors;
mod eval;
mod types;
#[allow(dead_code, unused_imports, unused_variables, unused_assignments, non_upper_case_globals, non_snake_case, unreachable_code, unused_parens, unused_mut)]
mod parser;
#[allow(dead_code, unused_imports, unused_variables, unused_assignments, non_upper_case_globals, non_snake_case, unreachable_code, unused_parens, unused_mut)]
mod lexer;

use std::collections::HashMap;

use crate::eval::{is_zero, Ctx, Value};
use crate::lexer::Lexer;
use crate::parser::Parser;

fn parse(input: &str) -> ast::Expr {
    let lexer = Lexer::new(input.as_bytes());
    let parser = Parser::new(lexer);
    parser.do_parse().unwrap_or_else(|| {
        eprintln!("parse error: {}", input);
        std::process::exit(1);
    })
}

// ansi helpers
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";

fn run_test(rule: &str, kv: HashMap<String, Value>) {
    let expr = parse(rule);
    let ctx = Ctx { kv: kv.clone() };
    let result = expr.eval(&ctx);

    // rule
    println!("  {DIM}rule:{RESET}   {BOLD}{}{RESET}", rule);

    // kv (compact)
    let pairs: Vec<String> = kv
        .iter()
        .map(|(k, v)| format!("{CYAN}{}{RESET}={}", k, v))
        .collect();
    println!("  {DIM}kv:{RESET}     {{{}}}", pairs.join(", "));

    // result
    match &result.error {
        Some(e) => println!("  {DIM}result:{RESET} {YELLOW}ERROR{RESET} {DIM}({e}){RESET}"),
        None => {
            let pass = !is_zero(&result.value);
            if pass {
                println!("  {DIM}result:{RESET} {GREEN}PASS{RESET} {DIM}({value}){RESET}", value = result.value);
            } else {
                println!("  {DIM}result:{RESET} {RED}FAIL{RESET} {DIM}({value}){RESET}", value = result.value);
            }
        }
    }
    println!();
}

macro_rules! kv {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = HashMap::new();
        $(m.insert($k.to_string(), $v);)*
        m
    }};
}

fn main() {
    // If args given, parse+eval that expression with an empty context
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let input = args.join(" ");
        run_test(&input, HashMap::new());
        return;
    }

    println!("{BOLD}=== rulekit eval tests ==={RESET}\n");

    // --- comparisons ---
    println!("{BOLD}{CYAN}--- comparisons ---{RESET}\n");

    run_test("port == 8080", kv! {
        "port" => Value::Int(8080),
    });

    run_test("port == 8080", kv! {
        "port" => Value::Int(9090),
    });

    run_test("status != 200", kv! {
        "status" => Value::Int(404),
    });

    run_test("score > 90", kv! {
        "score" => Value::Float(95.5),
    });

    run_test("score <= 50", kv! {
        "score" => Value::Int(50),
    });

    // --- strings ---
    println!("{BOLD}{CYAN}--- strings ---{RESET}\n");

    run_test(r#"host == "example.com""#, kv! {
        "host" => Value::String("example.com".into()),
    });

    run_test(r#"path contains "/api""#, kv! {
        "path" => Value::String("/api/v1/users".into()),
    });

    run_test(r#"path contains "/admin""#, kv! {
        "path" => Value::String("/api/v1/users".into()),
    });

    // --- regex ---
    println!("{BOLD}{CYAN}--- regex ---{RESET}\n");

    run_test(r"domain matches /\.com$/", kv! {
        "domain" => Value::String("example.com".into()),
    });

    run_test(r"domain matches /\.org$/", kv! {
        "domain" => Value::String("example.com".into()),
    });

    // --- boolean logic ---
    println!("{BOLD}{CYAN}--- boolean logic ---{RESET}\n");

    run_test("active and verified", kv! {
        "active" => Value::Bool(true),
        "verified" => Value::Bool(true),
    });

    run_test("active and verified", kv! {
        "active" => Value::Bool(true),
        "verified" => Value::Bool(false),
    });

    run_test("active or verified", kv! {
        "active" => Value::Bool(false),
        "verified" => Value::Bool(true),
    });

    run_test("not blocked", kv! {
        "blocked" => Value::Bool(false),
    });

    // --- in (arrays & cidr) ---
    println!("{BOLD}{CYAN}--- in ---{RESET}\n");

    run_test(r#"method in ["GET", "POST"]"#, kv! {
        "method" => Value::String("POST".into()),
    });

    run_test(r#"method in ["GET", "POST"]"#, kv! {
        "method" => Value::String("DELETE".into()),
    });

    run_test("ip in 10.0.0.0/8", kv! {
        "ip" => Value::Ip("10.1.2.3".parse().unwrap()),
    });

    run_test("ip in 10.0.0.0/8", kv! {
        "ip" => Value::Ip("192.168.1.1".parse().unwrap()),
    });

    // --- ip ---
    println!("{BOLD}{CYAN}--- ip ---{RESET}\n");

    run_test("src == 192.168.1.1", kv! {
        "src" => Value::Ip("192.168.1.1".parse().unwrap()),
    });

    // --- missing fields ---
    println!("{BOLD}{CYAN}--- missing fields ---{RESET}\n");

    run_test("port == 8080", kv! {});

    // --- literal-only ---
    println!("{BOLD}{CYAN}--- literals ---{RESET}\n");

    run_test("5 > 3", kv! {});
    run_test("true and false", kv! {});
    run_test(r#""hello" == "hello""#, kv! {});
}

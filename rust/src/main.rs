mod ast;
mod types;
#[allow(dead_code, unused_imports, unused_variables, unused_assignments, non_upper_case_globals, non_snake_case, unreachable_code, unused_parens, unused_mut)]
mod parser;
#[allow(dead_code, unused_imports, unused_variables, unused_assignments, non_upper_case_globals, non_snake_case, unreachable_code, unused_parens, unused_mut)]
mod lexer;

use crate::lexer::Lexer;
use crate::parser::Parser;

fn main() {
    let input = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if input.is_empty() {
        eprintln!("usage: rulekit <expression>");
        std::process::exit(1);
    }

    let lexer = Lexer::new(input.as_bytes());
    let parser = Parser::new(lexer);
    match parser.do_parse() {
        Some(expr) => println!("{:#?}", expr),
        None => {
            eprintln!("parse error");
            std::process::exit(1);
        }
    }
}

//! A small program used by bench/size.sh to measure compiled size.

use rulekit::{JsonOptions, KvInput, Opts};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rule = rulekit::parse(args.get(1).map_or("a == 1", String::as_str)).expect("parse");
    let kv = rulekit::decode_json::<()>(br#"{"a": 1}"#, JsonOptions::default()).expect("json");
    let input = KvInput::new(kv);
    let trace = args.len() > 2;
    let result = rule.eval(&input, &(), Opts::default().with_trace(trace));
    println!("{}", result.pass());
}

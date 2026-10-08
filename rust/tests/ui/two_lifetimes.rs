#[derive(rulekit::Args)]
struct Args<'a, 'b> {
    a: &'a str,
    b: &'b str,
}
fn main() {}

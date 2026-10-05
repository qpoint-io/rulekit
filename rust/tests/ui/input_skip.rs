#[derive(rulekit::Input)]
struct Req {
    #[rulekit(skip)]
    secret: String,
}
fn main() {}

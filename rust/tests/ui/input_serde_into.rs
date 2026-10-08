#[derive(rulekit::Input)]
#[serde(into = "String")]
struct Req {
    name: String,
}
fn main() {}

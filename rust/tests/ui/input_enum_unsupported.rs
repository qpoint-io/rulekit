struct Nope;

#[derive(rulekit::Input)]
#[serde(tag = "type", content = "payload")]
enum Event {
    Started { pid: u32 },
    Other(Nope),
}
fn main() {}

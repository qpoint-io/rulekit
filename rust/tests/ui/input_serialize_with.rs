fn upper<S>(_: &str, _: S) -> Result<(), ()> {
    Ok(())
}

#[derive(rulekit::Input)]
struct Req {
    #[serde(serialize_with = "upper")]
    name: String,
}
fn main() {}

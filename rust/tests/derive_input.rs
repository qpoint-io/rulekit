//! `#[derive(Input)]` reads only the fields a rule touches.

use std::collections::HashMap;

use rulekit::ast::Segment;
use rulekit::value::Val;
use rulekit::{BoxError, InputValue, Opts};

struct Boom;

impl<C: ?Sized> InputValue<C> for Boom {
    fn get<'a>(&'a self, _: &'a C, _: &[Segment]) -> Result<Option<Val<'a>>, BoxError> {
        panic!("untouched field was read");
    }
}

#[derive(rulekit::Input)]
struct User<'a> {
    id: u64,
    name: &'a str,
}

#[derive(rulekit::Input)]
struct Req<'a, T> {
    host: &'a str,
    port: u16,
    #[rulekit(rename = "user-agent")]
    ua: Option<&'a str>,
    #[rulekit(skip)]
    secret: String,
    tags: Vec<String>,
    user: User<'a>,
    headers: HashMap<String, String>,
    extra: T,
    boom: Boom,
}

#[test]
fn derived_struct_reads_only_touched_fields() {
    let req = Req {
        host: "api.acme.com",
        port: 8443,
        ua: Some("curl"),
        secret: "nope".into(),
        tags: vec!["db".into(), "api".into()],
        user: User {
            id: 42,
            name: "ada",
        },
        headers: HashMap::from([("x-env".into(), "prod".into())]),
        extra: 7u32,
        boom: Boom,
    };
    let rule = rulekit::parse(
        r#"host == "api.acme.com" and port == 8443 and user-agent == "curl" and tags contains "db" and tags[1] == "api" and user.id == 42 and user.name == "ada" and headers["x-env"] == "prod" and extra == 7"#,
    )
    .unwrap();
    assert!(rule.eval(&(), &req, Opts::default()).pass());

    let missing = rulekit::parse("secret == \"nope\" or absent == 1").unwrap();
    assert_eq!(
        missing
            .eval(&(), &req, Opts::default())
            .missing_fields()
            .collect::<Vec<_>>(),
        ["secret", "absent"]
    );
    assert_eq!(req.secret, "nope");
}

#[test]
fn derived_slice_field_is_a_list() {
    #[derive(rulekit::Input)]
    struct Row<'a> {
        tags: &'a [String],
    }
    let tags = ["db".to_owned(), "api".to_owned()];
    let row = Row { tags: &tags };
    let rule = rulekit::parse(r#"tags contains "api" and tags[0] == "db""#).unwrap();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
}

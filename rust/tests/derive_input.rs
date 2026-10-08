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

    let missing = rulekit::parse("absent == 1").unwrap();
    assert_eq!(
        missing
            .eval(&(), &req, Opts::default())
            .missing_fields()
            .collect::<Vec<_>>(),
        ["absent"]
    );
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

#[test]
fn bytes_attribute_is_a_byte_string() {
    use std::borrow::Cow;

    #[derive(rulekit::Input)]
    struct Row<'a> {
        #[rulekit(bytes)]
        body: &'a [u8],
        #[rulekit(bytes)]
        owned: Vec<u8>,
        #[rulekit(bytes)]
        fixed: [u8; 4],
        #[rulekit(bytes)]
        boxed: Box<[u8]>,
        #[rulekit(bytes)]
        cow: Cow<'a, [u8]>,
        nums: Vec<u8>,
        raw: bytes::Bytes,
        buf: serde_bytes::ByteBuf,
        view: &'a serde_bytes::Bytes,
        array: serde_bytes::ByteArray<4>,
    }

    let view = serde_bytes::Bytes::new(b"POST");
    let row = Row {
        body: b"POST",
        owned: b"POST".to_vec(),
        fixed: *b"POST",
        boxed: b"POST".to_vec().into_boxed_slice(),
        cow: Cow::Borrowed(b"POST"),
        nums: b"POST".to_vec(),
        raw: bytes::Bytes::from_static(b"POST"),
        buf: serde_bytes::ByteBuf::from(b"POST".to_vec()),
        view,
        array: serde_bytes::ByteArray::new(*b"POST"),
    };
    let rule = rulekit::parse(
        r#"body == "POST" and owned == "POST" and fixed == "POST" and boxed == "POST" and cow == "POST" and raw == "POST" and buf == "POST" and view == "POST" and array == "POST" and nums[0] == 80"#,
    )
    .unwrap();
    assert!(rule.eval(&(), &row, Opts::default()).pass());

    let indexed = rulekit::parse("body[0] == 80").unwrap();
    assert!(indexed.eval(&(), &row, Opts::default()).unknown());
    let as_text = rulekit::parse(r#"nums == "POST""#).unwrap();
    assert!(!as_text.eval(&(), &row, Opts::default()).pass());
}

#[test]
fn bytes_mut_is_a_byte_string() {
    #[derive(rulekit::Input)]
    struct Row {
        raw: bytes::BytesMut,
    }
    let row = Row {
        raw: bytes::BytesMut::from(&b"POST"[..]),
    };
    let rule = rulekit::parse(r#"raw == "POST""#).unwrap();
    assert!(rule.eval(&(), &row, Opts::default()).pass());
}

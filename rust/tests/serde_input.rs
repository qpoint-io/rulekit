//! `SerdeInput` reads any `Serialize` value with serde's names and layout.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicUsize, Ordering};

use ipnet::{IpNet, Ipv4Net};
use rulekit::ast::Segment;
use rulekit::value::{Cidr, Ip, Mac, Url, Val, ValueRef};
use rulekit::{Input, InputValue, Opts, SerdeInput, kv};
use serde::Serialize;
use serde::ser::{self, SerializeStruct, Serializer};
use serde_json::{Value as Json, json};

#[derive(Debug, PartialEq)]
enum Outcome {
    Pass,
    Fail,
    Missing(Vec<String>),
    Error(String),
}

fn outcome<T: Serialize + ?Sized>(value: &T, rule: &str) -> Outcome {
    let parsed = rulekit::parse(rule).unwrap_or_else(|err| panic!("{rule}: {err}"));
    let opts = Opts::default();
    let input = SerdeInput::new(value);
    let result = parsed.eval(&input, &opts);
    if let Some(err) = result.error() {
        Outcome::Error(err.to_string())
    } else if result.unknown() {
        Outcome::Missing(result.missing_fields().map(str::to_owned).collect())
    } else if result.pass() {
        Outcome::Pass
    } else {
        Outcome::Fail
    }
}

#[track_caller]
fn pass<T: Serialize + ?Sized>(value: &T, rule: &str) {
    assert_eq!(outcome(value, rule), Outcome::Pass, "{rule}");
}

#[track_caller]
fn fail<T: Serialize + ?Sized>(value: &T, rule: &str) {
    assert_eq!(outcome(value, rule), Outcome::Fail, "{rule}");
}

#[track_caller]
fn missing<T: Serialize + ?Sized>(value: &T, rule: &str, fields: &[&str]) {
    let want = Outcome::Missing(fields.iter().map(|f| (*f).to_owned()).collect());
    assert_eq!(outcome(value, rule), want, "{rule}");
}

fn key(k: &str) -> Segment {
    Segment::Key {
        key: k.to_owned(),
        bracket: false,
    }
}

// --- names ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Process {
    exe_path: String,
    #[serde(rename = "user-name")]
    user_name: String,
    #[serde(rename(serialize = "ser_only", deserialize = "de_only"))]
    both: bool,
    r#type: String,
}

#[test]
fn rename_and_rename_all_give_the_wire_names() {
    let process = Process {
        exe_path: "/bin/sh".into(),
        user_name: "ada".into(),
        both: true,
        r#type: "raw".into(),
    };
    pass(
        &process,
        r#"exePath == "/bin/sh" and ["user-name"] == "ada" and ser_only and type == "raw""#,
    );
    missing(&process, "exe_path == 1", &["exe_path"]);
    missing(&process, "user_name == 1", &["user_name"]);
    missing(&process, "de_only", &["de_only"]);
}

// --- enum layouts ---

#[derive(Serialize)]
enum Status {
    Ok,
    Err(String),
    Pair(u8, String),
    Move { x: i32, y: i32 },
}

#[derive(Serialize)]
struct Job {
    status: Status,
}

#[test]
fn externally_tagged_variants() {
    pass(&Job { status: Status::Ok }, r#"status == "Ok""#);
    missing(&Job { status: Status::Ok }, "status.Err", &["status.Err"]);

    let err = Job {
        status: Status::Err("boom".into()),
    };
    pass(&err, r#"status.Err == "boom""#);
    missing(&err, "status.Ok", &["status.Ok"]);
    missing(&err, "status.Move.x == 1", &["status.Move.x"]);

    let pair = Job {
        status: Status::Pair(7, "seven".into()),
    };
    pass(
        &pair,
        r#"status.Pair[0] == 7 and status.Pair[1] == "seven""#,
    );
    missing(&pair, "status.Pair[2]", &["status.Pair[2]"]);

    let step = Job {
        status: Status::Move { x: 1, y: -2 },
    };
    pass(&step, "status.Move.x == 1 and status.Move.y == -2");
    missing(&step, "status.Move.z", &["status.Move.z"]);
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Shape {
    Circle { r: u32 },
    Square { side: u32 },
    Unit,
    Wrapped(Inner),
}

#[derive(Serialize)]
struct Inner {
    n: u32,
}

#[test]
fn internally_tagged_variants() {
    let circle = Shape::Circle { r: 3 };
    pass(&circle, r#"kind == "circle" and r == 3"#);
    missing(&circle, "side == 3", &["side"]);
    pass(
        &Shape::Square { side: 4 },
        r#"kind == "square" and side == 4"#,
    );
    pass(&Shape::Unit, r#"kind == "unit""#);
    pass(
        &Shape::Wrapped(Inner { n: 5 }),
        r#"kind == "wrapped" and n == 5"#,
    );
}

#[derive(Serialize)]
#[serde(tag = "type", content = "payload")]
enum Message {
    Text(String),
    Count { n: u32 },
    Ping,
    Pair(u8, u8),
}

#[test]
fn adjacently_tagged_variants() {
    pass(
        &Message::Text("hi".into()),
        r#"type == "Text" and payload == "hi""#,
    );
    pass(
        &Message::Count { n: 2 },
        r#"type == "Count" and payload.n == 2"#,
    );
    pass(&Message::Pair(1, 2), "payload[0] == 1 and payload[1] == 2");
    pass(&Message::Ping, r#"type == "Ping""#);
    missing(&Message::Ping, "payload", &["payload"]);
}

#[derive(Serialize)]
#[serde(untagged)]
enum Either {
    A { a: u32 },
    B { b: String },
    C(u32),
    D,
}

#[derive(Serialize)]
struct Holder {
    v: Either,
}

#[test]
fn untagged_variants() {
    pass(&Either::A { a: 1 }, "a == 1");
    missing(&Either::A { a: 1 }, "b", &["b"]);
    pass(&Either::B { b: "x".into() }, r#"b == "x""#);
    pass(&Holder { v: Either::C(5) }, "v == 5");
    // A unit variant is `null`: present, and equal to nothing.
    fail(&Holder { v: Either::D }, "v == 5");
}

// --- flatten ---

#[derive(Serialize)]
struct Meta {
    source: String,
    level: u8,
}

#[derive(Serialize)]
struct Optional {
    tag: String,
}

#[derive(Serialize)]
struct Outer {
    id: u32,
    #[serde(flatten)]
    meta: Meta,
    #[serde(flatten)]
    extra: BTreeMap<String, String>,
    #[serde(flatten)]
    optional: Option<Optional>,
}

#[test]
fn flatten_reads_flattened_fields_at_the_top() {
    let outer = Outer {
        id: 1,
        meta: Meta {
            source: "api".into(),
            level: 2,
        },
        extra: BTreeMap::from([("region".into(), "eu".into())]),
        optional: None,
    };
    pass(
        &outer,
        r#"id == 1 and source == "api" and level == 2 and region == "eu""#,
    );
    // A flattened `None` writes nothing.
    missing(&outer, r#"tag == "x""#, &["tag"]);

    let outer = Outer {
        optional: Some(Optional { tag: "t".into() }),
        ..outer
    };
    pass(&outer, r#"tag == "t""#);
}

#[test]
fn first_of_two_equal_keys_wins() {
    let outer = Outer {
        id: 1,
        meta: Meta {
            source: "api".into(),
            level: 2,
        },
        extra: BTreeMap::from([("id".into(), "from-map".into())]),
        optional: None,
    };
    pass(&outer, "id == 1");
}

// --- skip_serializing_if, Option ---

#[derive(Serialize)]
struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip)]
    #[allow(dead_code)]
    hidden: u8,
    note: Option<String>,
    parent: Option<Inner>,
}

fn request(request_id: Option<&str>, tags: &[&str]) -> Request {
    Request {
        request_id: request_id.map(str::to_owned),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        hidden: 9,
        note: None,
        parent: None,
    }
}

#[test]
fn skipped_fields_are_absent_and_present_ones_read() {
    let absent = request(None, &[]);
    missing(&absent, r#"request_id == "r1""#, &["request_id"]);
    missing(&absent, r#"tags contains "a""#, &["tags"]);
    missing(&absent, "hidden == 9", &["hidden"]);

    let present = request(Some("r1"), &["a"]);
    pass(&present, r#"request_id == "r1" and tags contains "a""#);
}

#[test]
fn none_is_null_and_some_reads_through() {
    let mut req = request(None, &[]);
    // `None` is written as `null`: present, so the comparison completes.
    fail(&req, r#"note == "x""#);
    missing(&req, "parent.n == 1", &["parent.n"]);

    req.note = Some("x".into());
    req.parent = Some(Inner { n: 1 });
    pass(&req, r#"note == "x" and parent.n == 1"#);
}

// --- lists, maps, serde_json::Value ---

#[derive(Serialize)]
struct Collections {
    tags: Vec<String>,
    nums: Vec<u32>,
    ips: Vec<IpAddr>,
    headers: BTreeMap<String, String>,
    by_id: BTreeMap<u32, String>,
    items: Vec<Inner>,
}

fn collections() -> Collections {
    Collections {
        tags: vec!["db".into(), "api".into()],
        nums: vec![1, 2, 3],
        ips: vec!["10.0.0.1".parse().unwrap(), "192.168.0.1".parse().unwrap()],
        headers: BTreeMap::from([("x-env".into(), "prod".into())]),
        by_id: BTreeMap::from([(5, "five".into()), (6, "six".into())]),
        items: vec![Inner { n: 10 }, Inner { n: 20 }],
    }
}

#[test]
fn vec_index_and_membership() {
    let c = collections();
    pass(&c, r#"tags[0] == "db" and tags[1] == "api""#);
    missing(&c, r#"tags[2] == "x""#, &["tags[2]"]);
    pass(&c, r#"tags contains "api" and not (tags contains "nope")"#);
    pass(
        &c,
        r#"nums contains 3 and nums[2] == 3 and tags[0] in ["db", "x"]"#,
    );
    // Items keep their type: an array of IPs holds IPs.
    pass(&c, "ips contains 10.0.0.1 and ips[1] in 192.168.0.0/16");
    // Reading a list of objects whole is an array; its items read by path.
    pass(&c, "items and items[1].n == 20");
    missing(&c, "items[2].n == 1", &["items[2].n"]);
}

#[test]
fn map_keys_match_as_serde_json_writes_them() {
    let c = collections();
    pass(&c, r#"headers["x-env"] == "prod" and by_id["5"] == "five""#);
    missing(&c, r#"headers["other"] == "x""#, &[r#"headers["other"]"#]);
    missing(&c, r#"by_id["7"] == "x""#, &[r#"by_id["7"]"#]);
    // An object read whole is opaque and truthy.
    pass(&c, "headers");
}

#[test]
fn map_key_kinds() {
    #[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
    enum Level {
        Low,
        High,
    }
    #[derive(Serialize)]
    struct Keys {
        by_bool: BTreeMap<bool, u8>,
        by_char: BTreeMap<char, u8>,
        by_level: BTreeMap<Level, u8>,
        by_signed: BTreeMap<i64, u8>,
        by_pair: BTreeMap<(u8, u8), u8>,
    }
    let keys = Keys {
        by_bool: BTreeMap::from([(true, 1)]),
        by_char: BTreeMap::from([('x', 2)]),
        by_level: BTreeMap::from([(Level::Low, 3), (Level::High, 4)]),
        by_signed: BTreeMap::from([(-1, 5)]),
        by_pair: BTreeMap::from([((1, 2), 6)]),
    };
    pass(
        &keys,
        r#"by_bool["true"] == 1 and by_char["x"] == 2 and by_level.High == 4
           and by_level.Low == 3 and by_signed["-1"] == 5"#,
    );
    // serde_json cannot write a tuple key either.
    let Outcome::Error(message) = outcome(&keys, r#"by_pair["1"] == 6"#) else {
        panic!("expected an error");
    };
    assert!(message.contains("key must be a string"), "{message}");
}

#[derive(Serialize)]
struct WithJson {
    data: Json,
    object: serde_json::Map<String, Json>,
}

#[test]
fn serde_json_values_nest() {
    let value = WithJson {
        data: json!({"a": {"b": [1, {"c": "deep"}]}, "n": null, "f": 1.5, "big": u64::MAX}),
        object: serde_json::Map::from_iter([("k".to_owned(), json!([true]))]),
    };
    pass(
        &value,
        r#"data.a.b[0] == 1 and data.a.b[1].c == "deep" and data.f == 1.5 and data.big == 18446744073709551615"#,
    );
    fail(&value, "data.n");
    missing(&value, "data.zz", &["data.zz"]);
    missing(&value, "data.a.b[2]", &["data.a.b[2]"]);
    pass(&value, "object.k[0]");

    // A bare `Value` is an input as well.
    let json = json!({"a": [{"b": 2}]});
    pass(&json, "a[0].b == 2");
}

// --- custom Serialize impls ---

struct Temp(u32);

impl Serialize for Temp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The string lives only for this call: the leaf must be copied out.
        let text = format!("temp-{}", self.0);
        serializer.serialize_str(&text)
    }
}

struct Custom(u32);

impl Serialize for Custom {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Custom", 2)?;
        let label = format!("label-{}", self.0);
        state.serialize_field("label", &label)?;
        state.serialize_field("temp", &Temp(self.0))?;
        state.end()
    }
}

#[derive(Serialize)]
struct Wrapper {
    temp: Temp,
    custom: Custom,
}

#[test]
fn custom_serialize_impls_work_with_temporaries() {
    let wrapper = Wrapper {
        temp: Temp(7),
        custom: Custom(3),
    };
    pass(
        &wrapper,
        r#"temp == "temp-7" and custom.label == "label-3" and custom.temp == "temp-3""#,
    );
}

struct Failing;

impl Serialize for Failing {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(<S::Error as ser::Error>::custom("cannot serialize"))
    }
}

#[test]
fn a_serialize_error_is_an_input_error() {
    #[derive(Serialize)]
    struct Holds {
        bad: Failing,
        fine: u32,
    }
    let value = Holds {
        bad: Failing,
        fine: 1,
    };
    pass(&value, "fine == 1");
    let Outcome::Error(message) = outcome(&value, "bad == 1") else {
        panic!("expected an error");
    };
    assert!(message.contains("cannot serialize"), "{message}");
}

// --- scalars ---

#[test]
fn integers_floats_and_bytes() {
    #[derive(Serialize)]
    struct Numbers {
        small: i8,
        wide: u64,
        negative: i64,
        float: f32,
        huge: u128,
        too_big: u128,
        bytes: serde_bytes::ByteBuf,
        list: Vec<u8>,
        letter: char,
    }
    let value = Numbers {
        small: -3,
        wide: u64::MAX,
        negative: i64::MIN,
        float: 0.5,
        huge: u128::from(u64::MAX),
        too_big: u128::MAX,
        bytes: serde_bytes::ByteBuf::from(vec![1, 2, 3]),
        list: vec![1, 2, 3],
        letter: 'q',
    };
    pass(
        &value,
        r#"small == -3 and wide == 18446744073709551615 and negative < 0 and float == 0.5
           and huge == 18446744073709551615 and letter == "q" and list[2] == 3 and list contains 1"#,
    );

    let input = SerdeInput::new(&value);
    let bytes = Input::get(&input, &[key("bytes")]).unwrap().unwrap();
    assert!(matches!(bytes.as_ref(), ValueRef::Bytes([1, 2, 3])));

    let Outcome::Error(message) = outcome(&value, "too_big == 1") else {
        panic!("expected an out-of-range error");
    };
    assert!(message.contains("out of range"), "{message}");
    // Not reading the field is not an error.
    pass(&value, "small == -3");
}

/// `serde_json`'s `arbitrary_precision` writes a number as a struct holding
/// its digits.
struct DigitsNumber(&'static str);

impl Serialize for DigitsNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("$serde_json::private::Number", 1)?;
        state.serialize_field("$serde_json::private::Number", self.0)?;
        state.end()
    }
}

#[test]
fn arbitrary_precision_numbers_are_numbers() {
    #[derive(Serialize)]
    struct Digits {
        int: DigitsNumber,
        wide: DigitsNumber,
        float: DigitsNumber,
    }
    let value = Digits {
        int: DigitsNumber("-42"),
        wide: DigitsNumber("18446744073709551615"),
        float: DigitsNumber("1.5"),
    };
    pass(
        &value,
        "int == -42 and wide == 18446744073709551615 and float == 1.5",
    );
}

// --- typed leaves ---

#[derive(Serialize)]
struct Conn {
    src: IpAddr,
    text: String,
    maybe: Option<IpAddr>,
    v4: Ipv4Addr,
    v6: Ipv6Addr,
    net: IpNet,
    net4: Ipv4Net,
}

fn conn() -> Conn {
    Conn {
        src: "10.1.2.3".parse().unwrap(),
        text: "10.1.2.3".into(),
        maybe: Some("192.168.1.1".parse().unwrap()),
        v4: "172.16.0.1".parse().unwrap(),
        v6: "fe80::1".parse().unwrap(),
        net: "10.0.0.0/8".parse().unwrap(),
        net4: "192.168.0.0/16".parse().unwrap(),
    }
}

#[test]
fn ip_fields_are_typed_not_strings() {
    let c = conn();
    pass(&c, "src in 10.0.0.0/8");
    // An IP is stringable, so it compares with its text too.
    pass(&c, r#"src == "10.1.2.3" and src == 10.1.2.3"#);
    pass(&c, r#"src.version == "v4" and v6.version == "v6""#);
    // The same text in a `String` is a string, not an IP.
    fail(&c, "text in 10.0.0.0/8");
    pass(&c, r#"text == "10.1.2.3""#);
    pass(&c, "v4 in 172.16.0.0/12 and v6 in fe80::/10");
}

#[test]
fn option_of_a_typed_leaf_is_typed() {
    let mut c = conn();
    pass(&c, "maybe in 192.168.0.0/16 and maybe.version == \"v4\"");
    c.maybe = None;
    fail(&c, "maybe in 192.168.0.0/16");
    missing(&c, "maybe.version == \"v4\"", &["maybe.version"]);
}

#[derive(Serialize)]
struct Containers<'a> {
    list: Vec<IpAddr>,
    options: Vec<Option<IpAddr>>,
    by_name: BTreeMap<String, IpAddr>,
    borrowed: &'a IpAddr,
    borrowed_list: Vec<&'a IpAddr>,
    nets: Vec<IpNet>,
    boxed: Box<IpAddr>,
}

#[test]
fn typed_leaves_inside_containers_and_references() {
    let (a, b): (IpAddr, IpAddr) = ("10.1.1.1".parse().unwrap(), "192.168.1.1".parse().unwrap());
    let c = Containers {
        list: vec![a, b],
        options: vec![Some(a), None],
        by_name: BTreeMap::from([("gw".into(), b)]),
        borrowed: &a,
        borrowed_list: vec![&a, &b],
        nets: vec!["10.0.0.0/8".parse().unwrap()],
        boxed: Box::new(a),
    };
    pass(&c, "list[0] in 10.0.0.0/8 and list[1] in 192.168.0.0/16");
    pass(
        &c,
        "options[0] in 10.0.0.0/8 and options[0].version == \"v4\"",
    );
    missing(&c, "options[1].version == \"v4\"", &["options[1].version"]);
    pass(&c, "by_name.gw in 192.168.0.0/16");
    pass(
        &c,
        "borrowed in 10.0.0.0/8 and borrowed_list[1] in 192.168.0.0/16",
    );
    pass(&c, "nets[0].prefix == 8");
    // A `Box` forwards to the inner `Serialize`, which writes text.
    fail(&c, "boxed in 10.0.0.0/8");
    pass(&c, r#"boxed == "10.1.1.1""#);
}

#[derive(Serialize)]
struct Newtype(IpAddr);

#[derive(Serialize)]
#[serde(transparent)]
struct Transparent(IpAddr);

#[derive(Serialize)]
enum External {
    At(IpAddr),
}

#[derive(Serialize)]
#[serde(untagged)]
enum Untagged {
    At(IpAddr),
}

#[derive(Serialize)]
#[serde(tag = "t", content = "c")]
enum Adjacent {
    At(IpAddr),
}

#[derive(Serialize)]
struct Wrapped {
    newtype: Newtype,
    transparent: Transparent,
    external: External,
    untagged: Untagged,
    adjacent: Adjacent,
}

#[test]
fn typed_leaves_through_newtypes_and_variants() {
    let ip: IpAddr = "10.1.1.1".parse().unwrap();
    let w = Wrapped {
        newtype: Newtype(ip),
        transparent: Transparent(ip),
        external: External::At(ip),
        untagged: Untagged::At(ip),
        adjacent: Adjacent::At(ip),
    };
    // serde passes the inner value to the serializer: typed.
    pass(
        &w,
        "newtype in 10.0.0.0/8 and external.At in 10.0.0.0/8 and adjacent.c in 10.0.0.0/8",
    );
    // These forward to the inner `serialize`, so only its text arrives.
    fail(&w, "transparent in 10.0.0.0/8");
    fail(&w, "untagged in 10.0.0.0/8");
    pass(
        &w,
        r#"transparent == "10.1.1.1" and untagged == "10.1.1.1""#,
    );
}

#[test]
fn network_fields_are_typed() {
    let c = conn();
    pass(
        &c,
        "net.prefix == 8 and net.network == 10.0.0.0 and net.version == \"v4\"",
    );
    pass(&c, r#"net == "10.0.0.0/8" and net4.prefix == 16"#);
}

#[derive(Serialize)]
struct Typed {
    ip: Ip,
    cidr: Cidr,
    mac: Mac,
    url: Url,
}

#[test]
fn rulekit_values_are_typed() {
    let typed = Typed {
        ip: Ip::parse("10.1.2.3").unwrap(),
        cidr: Cidr::parse("10.0.0.0/8").unwrap(),
        mac: Mac::parse("00:1a:2b:3c:4d:5e").unwrap(),
        url: Url::parse("https://example.com:8443/a?env=prod").unwrap(),
    };
    pass(
        &typed,
        r#"ip in 10.0.0.0/8 and ip.version == "v4" and cidr.prefix == 8
           and mac.oui == 00:1a:2b and url.host == "example.com" and url.port == 8443
           and url.query.env == "prod" and url == "https://example.com:8443/a?env=prod""#,
    );
}

#[cfg(feature = "url")]
#[test]
fn url_crate_fields_and_whole_value() {
    #[derive(Serialize)]
    struct Page {
        url: ::url::Url,
        other: Option<::url::Url>,
    }
    let page = Page {
        url: "https://Example.com:8443/a?env=prod#top".parse().unwrap(),
        other: None,
    };
    pass(
        &page,
        r#"url.scheme == "https" and url.host == "example.com" and url.port == 8443
           and url.path == "/a" and url.query.env == "prod" and url.fragment == "top""#,
    );
    pass(
        &page,
        r#"url == "https://example.com:8443/a?env=prod#top" and starts_with(url, "https://")"#,
    );
    missing(&page, r#"other.host == "x""#, &["other.host"]);
}

#[test]
fn a_typed_leaf_is_the_input_itself() {
    let addr: IpAddr = "10.9.9.9".parse().unwrap();
    pass(&addr, r#"version == "v4""#);
    let input = SerdeInput::new(&addr);
    let got = Input::get(&input, &[]).unwrap().unwrap();
    assert!(matches!(got.as_ref(), ValueRef::Ip(ip) if ip == Ip::parse("10.9.9.9").unwrap()));
}

// --- laziness ---

struct Poison;

impl Serialize for Poison {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        panic!("a sibling of the path was serialized");
    }
}

#[derive(Serialize)]
struct Siblings {
    before: Poison,
    wanted: u32,
    after: Poison,
    nested: Nested,
    list: Vec<Poison>,
    map: BTreeMap<String, Poison>,
    tagged: Tagged,
}

#[derive(Serialize)]
struct Nested {
    first: Poison,
    inner: u32,
    last: Poison,
}

#[derive(Serialize)]
#[serde(tag = "t", content = "c")]
enum Tagged {
    #[allow(dead_code)]
    Other(Poison),
    Mine {
        a: Poison,
        b: u32,
    },
}

fn siblings() -> Siblings {
    Siblings {
        before: Poison,
        wanted: 7,
        after: Poison,
        nested: Nested {
            first: Poison,
            inner: 8,
            last: Poison,
        },
        list: vec![],
        map: BTreeMap::new(),
        tagged: Tagged::Mine { a: Poison, b: 9 },
    }
}

#[test]
fn siblings_off_the_path_are_never_serialized() {
    let value = siblings();
    pass(&value, "wanted == 7");
    pass(&value, "nested.inner == 8");
    pass(&value, "tagged.c.b == 9");
    pass(&value, r#"tagged.t == "Mine""#);
    missing(&value, "nested.missing", &["nested.missing"]);
    missing(&value, "list[0]", &["list[0]"]);
    missing(&value, "map.k", &["map.k"]);
}

#[test]
fn list_and_map_items_off_the_path_are_never_serialized() {
    #[derive(Serialize)]
    struct Lists {
        items: Vec<Item>,
        by_name: BTreeMap<&'static str, Item>,
    }
    #[derive(Serialize)]
    #[serde(untagged)]
    enum Item {
        Value(u32),
        Poison(Poison),
    }
    let value = Lists {
        items: vec![Item::Poison(Poison), Item::Value(2), Item::Poison(Poison)],
        by_name: BTreeMap::from([
            ("a", Item::Poison(Poison)),
            ("b", Item::Value(3)),
            ("c", Item::Poison(Poison)),
        ]),
    };
    pass(&value, "items[1] == 2 and by_name.b == 3");
}

/// Counts how many times each field is serialized.
struct Counted<'a>(&'a AtomicUsize, u32);

impl Serialize for Counted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        serializer.serialize_u32(self.1)
    }
}

#[test]
fn only_the_target_is_serialized() {
    #[derive(Serialize)]
    struct Fields<'a> {
        a: Counted<'a>,
        b: Counted<'a>,
        c: Counted<'a>,
    }
    let calls = AtomicUsize::new(0);
    let value = Fields {
        a: Counted(&calls, 1),
        b: Counted(&calls, 2),
        c: Counted(&calls, 3),
    };
    pass(&value, "b == 2");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    pass(&value, "c == 3");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    missing(&value, "d == 4", &["d"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

// --- whole values ---

#[test]
fn whole_values() {
    #[derive(Serialize)]
    struct Whole {
        object: Inner,
        pairs: (u8, String),
        unit: (),
        empty: Vec<u8>,
    }
    let value = Whole {
        object: Inner { n: 1 },
        pairs: (1, "x".into()),
        unit: (),
        empty: vec![],
    };
    // A struct read whole is an opaque object: truthy, equal to nothing.
    pass(&value, "object");
    // A tuple is an array.
    pass(&value, r#"pairs contains "x" and pairs[0] == 1"#);
    fail(&value, "unit");
    fail(&value, "empty");
    missing(&value, "object.zz", &["object.zz"]);
}

#[test]
fn top_level_sequences_are_inputs() {
    let list = vec!["a", "b"];
    let input = SerdeInput::new(&list);
    let second = Input::get(&input, &[Segment::Index(1)]).unwrap().unwrap();
    assert!(matches!(second.as_ref(), ValueRef::Str("b")));
    assert!(Input::get(&input, &[Segment::Index(2)]).unwrap().is_none());
}

// --- nesting ---

#[test]
fn serde_input_nests_in_other_inputs() {
    let event = Shape::Circle { r: 3 };
    let input = kv! {
        "service" => "api",
        "event" => SerdeInput::new(&event),
    };
    let rule =
        rulekit::parse(r#"service == "api" and event.kind == "circle" and event.r == 3"#).unwrap();
    assert!(rule.eval(&input, &Opts::default()).pass());

    let miss = rulekit::parse("event.side == 1").unwrap();
    let opts = Opts::default();
    let result = miss.eval(&input, &opts);
    assert_eq!(result.missing_fields().collect::<Vec<_>>(), ["event.side"]);
}

#[test]
fn serde_input_is_an_input_value() {
    let event = Inner { n: 4 };
    let input = SerdeInput::new(&event);
    let val = InputValue::get(&input, &[key("n")]).unwrap().unwrap();
    assert!(matches!(val, Val::Owned(_)));
    assert!(matches!(val.as_ref(), ValueRef::Int(4)));
    assert!(InputValue::get(&input, &[key("zz")]).unwrap().is_none());
}

// --- wire parity ---

mod wire {
    use super::*;

    #[derive(Serialize)]
    #[serde(transparent)]
    pub struct Timestamp(pub String);

    #[derive(Serialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Severity {
        Warn,
    }

    #[derive(Serialize)]
    #[serde(untagged)]
    pub enum Record {
        Entity(EntityRecord),
        Host(HostRecord),
    }

    #[derive(Serialize)]
    pub struct EntityRecord {
        pub timestamp: Timestamp,
        pub severity: Severity,
        #[serde(flatten)]
        pub event: EntityEvent,
    }

    #[derive(Serialize)]
    pub struct HostRecord {
        pub timestamp: Timestamp,
        #[serde(flatten)]
        pub event: HostEvent,
    }

    #[derive(Serialize)]
    #[serde(untagged)]
    pub enum EntityEvent {
        Process(ProcessEvent),
        Llm(LlmEvent),
        Tool(ToolCall),
    }

    #[derive(Serialize)]
    #[serde(tag = "type", content = "payload")]
    pub enum ProcessEvent {
        #[serde(rename = "process.started")]
        Started(ProcessStarted),
        #[serde(rename = "process.stopped")]
        Stopped(ProcessStopped),
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ProcessStarted {
        pub pid: u32,
        pub exe_path: String,
        pub parent_pid: Option<u32>,
        pub argv: Vec<String>,
        #[serde(rename = "user-name")]
        pub user_name: String,
    }

    #[derive(Serialize)]
    pub struct ProcessStopped {
        pub pid: u32,
        pub status: ExitStatus,
    }

    #[derive(Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ExitStatus {
        Exited { code: i32 },
        Signaled { signal: i32 },
    }

    #[derive(Serialize)]
    #[serde(tag = "type", content = "payload")]
    pub enum LlmEvent {
        #[serde(rename = "llm.request")]
        Request(LlmRequest),
        #[serde(rename = "llm.heartbeat")]
        Heartbeat,
        #[serde(rename = "llm.pair")]
        Pair(u32, String),
        #[serde(rename = "llm.inline", rename_all = "PascalCase")]
        Inline { model_name: String },
        #[serde(untagged)]
        Raw(serde_json::Map<String, Json>),
    }

    #[derive(Serialize)]
    pub struct LlmRequest {
        pub model: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
        pub cost: f64,
        pub duration_ms: Option<u64>,
        pub raw: Option<serde_json::Map<String, Json>>,
        pub tags: Vec<String>,
    }

    #[derive(Serialize)]
    pub struct ToolCall {
        pub tool: Tool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub arguments: Option<serde_json::Map<String, Json>>,
    }

    #[derive(Serialize)]
    pub enum Tool {
        Builtin { name: String },
        Skill(String),
        Pair(u8, u8),
        Unknown,
        Vendored(Meta),
    }

    #[derive(Serialize)]
    pub struct Meta {
        pub vendor: String,
        pub version: u64,
    }

    #[derive(Serialize)]
    #[serde(tag = "type", content = "payload")]
    pub enum HostEvent {
        #[serde(rename = "installation.discovered")]
        Discovered(Installation),
    }

    #[derive(Serialize)]
    pub struct Installation {
        pub id: String,
        pub labels: BTreeMap<String, String>,
        pub steps: Vec<Meta>,
        #[serde(flatten)]
        pub extra: BTreeMap<String, Json>,
    }
}

/// What a lookup found, comparable across serde and JSON inputs.
#[derive(Debug, PartialEq)]
enum Found {
    Missing,
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(String),
    Array(Vec<Found>),
    Object,
}

fn found(value: ValueRef<'_>) -> Found {
    match value {
        ValueRef::Null => Found::Null,
        ValueRef::Bool(b) => Found::Bool(b),
        ValueRef::Int(n) => Found::Int(n),
        ValueRef::Uint(n) => Found::Uint(n),
        ValueRef::Float(n) => Found::Float(n),
        ValueRef::Str(s) => Found::Str(s.to_owned()),
        ValueRef::Array(items) => Found::Array(items.iter().map(found).collect()),
        ValueRef::Object(_) => Found::Object,
        other => panic!("unexpected value {other:?}"),
    }
}

/// Equal, except that a struct or map read whole is opaque where JSON has the
/// object: its paths are compared one by one.
fn same(read: &Found, wire: &Found) -> bool {
    match (read, wire) {
        (Found::Object, Found::Object) => true,
        (Found::Array(a), Found::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
        }
        _ => read == wire,
    }
}

fn lookup(input: &dyn Input, path: &[Segment]) -> Found {
    match input.get(path).expect("lookup") {
        None => Found::Missing,
        Some(val) => found(val.as_ref()),
    }
}

fn paths(value: &Json, prefix: &mut Vec<Segment>, out: &mut Vec<Vec<Segment>>) {
    out.push(prefix.clone());
    match value {
        Json::Object(map) => {
            for (k, v) in map {
                prefix.push(key(k));
                paths(v, prefix, out);
                prefix.pop();
            }
        }
        Json::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                prefix.push(Segment::Index(i));
                paths(v, prefix, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

/// `SerdeInput` reads every path of the `serde_json` form of `value` as
/// `serde_json::Value` does, and nothing one step past it.
fn assert_wire_parity<T: Serialize>(value: &T) {
    let json = serde_json::to_value(value).expect("serialize");
    let input = SerdeInput::new(value);
    let mut all = Vec::new();
    paths(&json, &mut Vec::new(), &mut all);
    for path in &all {
        let read = lookup(&input, path);
        let wire = lookup(&json, path);
        assert!(
            same(&read, &wire),
            "path {path:?}: {read:?} != {wire:?} in {json}"
        );
        for past in [key("zz_absent"), Segment::Index(999)] {
            let mut path = path.clone();
            path.push(past);
            assert_eq!(lookup(&input, &path), Found::Missing, "{path:?} in {json}");
        }
    }
}

fn wire_records() -> Vec<wire::Record> {
    use wire::*;
    let ts = || Timestamp("2026-01-01T00:00:00Z".into());
    let entity = |event| {
        Record::Entity(EntityRecord {
            timestamp: ts(),
            severity: Severity::Warn,
            event,
        })
    };
    let request = |request_id: Option<&str>, raw: Option<Json>| {
        LlmEvent::Request(LlmRequest {
            model: "m".into(),
            request_id: request_id.map(str::to_owned),
            cost: 0.25,
            duration_ms: Some(u64::MAX),
            raw: raw.and_then(|v| v.as_object().cloned()),
            tags: vec!["a".into(), "b".into()],
        })
    };
    vec![
        entity(EntityEvent::Process(ProcessEvent::Started(
            ProcessStarted {
                pid: 42,
                exe_path: "/bin/sh".into(),
                parent_pid: None,
                argv: vec!["sh".into(), "-c".into()],
                user_name: "ada".into(),
            },
        ))),
        entity(EntityEvent::Process(ProcessEvent::Stopped(
            ProcessStopped {
                pid: 42,
                status: ExitStatus::Signaled { signal: 9 },
            },
        ))),
        entity(EntityEvent::Process(ProcessEvent::Stopped(
            ProcessStopped {
                pid: 7,
                status: ExitStatus::Exited { code: -1 },
            },
        ))),
        entity(EntityEvent::Llm(request(
            Some("r1"),
            Some(json!({"k": [1, {"n": null}], "s": "v"})),
        ))),
        entity(EntityEvent::Llm(request(None, None))),
        entity(EntityEvent::Llm(LlmEvent::Heartbeat)),
        entity(EntityEvent::Llm(LlmEvent::Pair(5, "five".into()))),
        entity(EntityEvent::Llm(LlmEvent::Inline {
            model_name: "inline".into(),
        })),
        entity(EntityEvent::Llm(LlmEvent::Raw(
            json!({"type": "raw", "x": 1}).as_object().cloned().unwrap(),
        ))),
        entity(EntityEvent::Tool(ToolCall {
            tool: Tool::Builtin {
                name: "Bash".into(),
            },
            arguments: Some(json!({"cmd": "ls"}).as_object().cloned().unwrap()),
        })),
        entity(EntityEvent::Tool(ToolCall {
            tool: Tool::Skill("grill-me".into()),
            arguments: None,
        })),
        entity(EntityEvent::Tool(ToolCall {
            tool: Tool::Pair(1, 2),
            arguments: None,
        })),
        entity(EntityEvent::Tool(ToolCall {
            tool: Tool::Unknown,
            arguments: None,
        })),
        entity(EntityEvent::Tool(ToolCall {
            tool: Tool::Vendored(Meta {
                vendor: "acme".into(),
                version: 2,
            }),
            arguments: None,
        })),
        Record::Host(HostRecord {
            timestamp: ts(),
            event: HostEvent::Discovered(Installation {
                id: "i-1".into(),
                labels: BTreeMap::from([("team".into(), "core".into())]),
                steps: vec![Meta {
                    vendor: "v".into(),
                    version: 1,
                }],
                extra: BTreeMap::from([("note".into(), json!({"deep": [true]}))]),
            }),
        }),
    ]
}

#[test]
fn every_wire_path_reads_the_same() {
    for record in wire_records() {
        assert_wire_parity(&record);
    }
}

#[test]
fn rules_read_wire_names_of_nested_records() {
    let records = wire_records();
    pass(
        &records[0],
        r#"severity == "warn" and type == "process.started" and payload.exePath == "/bin/sh"
           and payload.argv[1] == "-c" and payload["user-name"] == "ada""#,
    );
    // `parent_pid` is `None`, written as `null`: present, so this completes.
    fail(&records[0], "payload.parentPid == 1");
    pass(
        &records[1],
        r#"payload.status.kind == "signaled" and payload.status.signal == 9"#,
    );
    pass(
        &records[3],
        r#"type == "llm.request" and payload.request_id == "r1" and payload.raw.k[0] == 1 and payload.raw.s == "v""#,
    );
    missing(
        &records[4],
        r#"payload.request_id == "r1""#,
        &["payload.request_id"],
    );
    pass(&records[6], r#"payload[0] == 5 and payload[1] == "five""#);
    pass(&records[8], r#"type == "raw""#);
    pass(&records[10], r#"tool.Skill == "grill-me""#);
    pass(
        &records[14],
        r#"payload.labels.team == "core" and payload.note.deep[0]"#,
    );
}

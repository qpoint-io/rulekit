//! Plain Rust values as evaluation input.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use ipnet::IpNet;
use rulekit::Opts;

#[test]
fn hashmap_borrows_strings() {
    let input = HashMap::from([
        ("host".to_owned(), "api.acme.com".to_owned()),
        ("env".to_owned(), "prod".to_owned()),
    ]);
    let rule = rulekit::parse(r#"host == "api.acme.com" and env == "prod""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
}

#[test]
fn vec_membership_and_index_do_not_copy() {
    let input = HashMap::from([("tags".to_owned(), vec!["db".to_owned(), "api".to_owned()])]);
    let rule = rulekit::parse(r#""db" in tags and tags[0] == "db" and tags[1] == "api""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
    let missing = rulekit::parse(r#""nope" in tags"#).unwrap();
    assert!(missing.eval(&(), &input, Opts::default()).fail());
}

#[test]
fn nested_map_lookup() {
    let headers = BTreeMap::from([("x-env".to_owned(), "prod".to_owned())]);
    let input = HashMap::from([("headers".to_owned(), headers)]);
    let rule = rulekit::parse(r#"headers["x-env"] == "prod" and headers.missing == "x""#).unwrap();
    let result = rule.eval(&(), &input, Opts::default());
    assert_eq!(
        result.missing_fields().collect::<Vec<_>>(),
        ["headers.missing"]
    );
}

#[test]
fn option_none_is_missing() {
    let input = HashMap::from([("host".to_owned(), None::<String>)]);
    let rule = rulekit::parse(r#"host == "x""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).unknown());
}

#[test]
fn ip_and_cidr_fields() {
    let input = HashMap::from([("src".to_owned(), "10.1.2.3".parse::<IpAddr>().unwrap())]);
    let rule = rulekit::parse(r#"src == 10.1.2.3 and src.version == "v4""#).unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());

    let nets = HashMap::from([("net".to_owned(), "10.0.0.0/8".parse::<IpNet>().unwrap())]);
    let rule = rulekit::parse(r#"net.prefix == 8 and net.version == "v4""#).unwrap();
    assert!(rule.eval(&(), &nets, Opts::default()).pass());
}

#[test]
fn json_value_is_walked_lazily() {
    let input = serde_json::json!({
        "host": "api.acme.com",
        "port": 8443,
        "tags": ["db", "api"],
        "headers": {"x-env": "prod"}
    });
    let rule = rulekit::parse(
        r#"host == "api.acme.com" and port == 8443 and "db" in tags and headers["x-env"] == "prod""#,
    )
    .unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
}

#[test]
fn vec_u8_is_a_list_of_numbers() {
    let input = HashMap::from([("bytes".to_owned(), vec![1u8, 2, 3])]);
    let rule = rulekit::parse("bytes[0] == 1 and 2 in bytes").unwrap();
    assert!(rule.eval(&(), &input, Opts::default()).pass());
}

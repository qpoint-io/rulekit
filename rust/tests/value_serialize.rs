//! Serializing rule values.

use rulekit::value::{ArrayRef, Cidr, Ip, Map, ObjectRef, Url, Value, ValueRef};
use serde_json::json;

#[test]
fn values_serialize_as_json_like_data() {
    let value = Value::Array(vec![
        Value::Null,
        Value::Bool(true),
        Value::Int(-1),
        Value::Uint(u64::MAX),
        Value::Float(0.5),
        Value::String("s".into()),
        Value::Bytes(vec![1, 2]),
        Value::Ip(Ip::parse("10.0.0.1").unwrap()),
        Value::Cidr(Cidr::parse("10.0.0.0/8").unwrap()),
        Value::Url(Box::new(Url::parse("https://example.com/a").unwrap())),
        Value::Regex(Box::new(regex::Regex::new("^a+$").unwrap())),
        Value::Object(Map::from_iter([(
            "k".to_owned(),
            Value::Array(vec![Value::Int(1)]),
        )])),
    ]);
    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        json!([
            null,
            true,
            -1,
            u64::MAX,
            0.5,
            "s",
            [1, 2],
            "10.0.0.1",
            "10.0.0.0/8",
            "https://example.com/a",
            "^a+$",
            {"k": [1]}
        ])
    );
}

#[test]
fn borrowed_lists_serialize_and_unlistable_objects_are_null() {
    let tags = vec!["a".to_owned(), "b".to_owned()];
    let list = ValueRef::Array(ArrayRef::List(&tags));
    assert_eq!(serde_json::to_value(list).unwrap(), json!(["a", "b"]));

    let opaque = ValueRef::Object(ObjectRef::Opaque);
    assert_eq!(serde_json::to_value(opaque).unwrap(), json!(null));
}

//! A random `Serialize` tree and an independent model of what `SerdeInput`
//! reads from it, shared by the proptest suite (`tests/serde_input_props.rs`)
//! and the libFuzzer target (`fuzz/fuzz_targets/serde_input.rs`).
//!
//! The tree puts every typed leaf type behind zero to three references, as
//! a newtype struct, `Some`, a sequence element, a struct field, or a map
//! value. The model reads a leaf behind at most two references as the leaf's
//! own `InputValue`, and one behind three as the string serde writes, which
//! is the whole contract of the `unsafe` downcast those reads go through.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use arbitrary::Arbitrary;
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use rulekit::ast::Segment;
use rulekit::value::{Cidr, Ip, Mac, Map, Url, Val, Value, ValueRef};
use rulekit::{InputValue, SerdeInput};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, Serializer};

const URLS: &[&str] = &[
    "https://example.com",
    "http://user:pw@10.0.0.1:8080/a/b?q=1&r=2#frag",
    "https://[2001:db8::1]/x",
    "ftp://files.example.org/pub/",
    "https://example.com:443/%7Euser",
];

/// Which leaf type, built from fuzzer bytes.
#[derive(Debug, Clone, Arbitrary)]
pub enum Seed {
    IpAddr(IpAddr),
    Ipv4Addr(Ipv4Addr),
    Ipv6Addr(Ipv6Addr),
    IpNet(IpAddr, u8),
    Ipv4Net(Ipv4Addr, u8),
    Ipv6Net(Ipv6Addr, u8),
    Ip(IpAddr),
    Cidr(IpAddr, u8),
    Mac([u8; 6]),
    Url(u8),
    UrlCrate(u8),
}

/// A leaf value of one of `SerdeInput`'s typed leaf types.
#[derive(Debug, Clone)]
pub enum Leaf {
    IpAddr(IpAddr),
    Ipv4Addr(Ipv4Addr),
    Ipv6Addr(Ipv6Addr),
    IpNet(IpNet),
    Ipv4Net(Ipv4Net),
    Ipv6Net(Ipv6Net),
    Ip(Ip),
    Cidr(Cidr),
    Mac(Mac),
    Url(Url),
    UrlCrate(url::Url),
}

impl From<&Seed> for Leaf {
    fn from(seed: &Seed) -> Leaf {
        let net = |addr: IpAddr, prefix: u8| {
            let max = if addr.is_ipv4() { 33 } else { 129 };
            IpNet::new(addr, prefix % max).unwrap()
        };
        let url = |i: u8| URLS[usize::from(i) % URLS.len()];
        match *seed {
            Seed::IpAddr(a) => Leaf::IpAddr(a),
            Seed::Ipv4Addr(a) => Leaf::Ipv4Addr(a),
            Seed::Ipv6Addr(a) => Leaf::Ipv6Addr(a),
            Seed::IpNet(a, p) => Leaf::IpNet(net(a, p)),
            Seed::Ipv4Net(a, p) => Leaf::Ipv4Net(Ipv4Net::new(a, p % 33).unwrap()),
            Seed::Ipv6Net(a, p) => Leaf::Ipv6Net(Ipv6Net::new(a, p % 129).unwrap()),
            Seed::Ip(a) => Leaf::Ip(Ip::from_addr(a)),
            Seed::Cidr(a, p) => Leaf::Cidr(Cidr::from_net(net(a, p))),
            Seed::Mac(b) => Leaf::Mac(Mac::from_bytes(&b).unwrap()),
            Seed::Url(i) => Leaf::Url(Url::parse(url(i)).unwrap()),
            Seed::UrlCrate(i) => Leaf::UrlCrate(url::Url::parse(url(i)).unwrap()),
        }
    }
}

/// How a node hands a leaf to the serializer.
#[derive(Debug, Clone, Copy, Arbitrary)]
pub enum Via {
    NewtypeStruct,
    Some,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Arbitrary)]
pub enum Field {
    A,
    B,
    C,
    D,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Field::A => "a",
            Field::B => "b",
            Field::C => "c",
            Field::D => "d",
        }
    }
}

/// The fuzzer's input: a tree of `Node`s.
#[derive(Debug, Clone, Arbitrary)]
pub enum Shape {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Leaf { seed: Seed, depth: u8, via: Via },
    List(Vec<Shape>),
    Struct(Vec<(Field, Shape)>),
    Map(Vec<(String, Shape)>),
    Boxed(Box<Shape>),
    Shared(Box<Shape>),
}

/// A `Serialize` tree built from a [`Shape`].
#[derive(Debug, Clone)]
pub enum Node {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    /// `depth` is 0 to 3: how many references the walk sees around the leaf.
    Leaf {
        leaf: Leaf,
        depth: u8,
        via: Via,
    },
    List(Vec<Node>),
    /// Fields with distinct names.
    Struct(Vec<(Field, Node)>),
    /// Entries with distinct keys.
    Map(Vec<(String, Node)>),
    /// `Box` forwards to the inner `serialize`.
    Boxed(Box<Node>),
    /// So does `Arc`.
    Shared(Arc<Node>),
}

impl From<&Shape> for Node {
    fn from(shape: &Shape) -> Node {
        match shape {
            Shape::Null => Node::Null,
            Shape::Bool(b) => Node::Bool(*b),
            Shape::Int(n) => Node::Int(*n),
            Shape::Str(s) => Node::Str(s.clone()),
            Shape::Leaf { seed, depth, via } => Node::Leaf {
                leaf: seed.into(),
                depth: depth % 4,
                via: *via,
            },
            Shape::List(items) => Node::List(items.iter().map(Node::from).collect()),
            Shape::Struct(fields) => {
                let mut out: Vec<(Field, Node)> = Vec::new();
                for (f, s) in fields {
                    if !out.iter().any(|(g, _)| g == f) {
                        out.push((*f, s.into()));
                    }
                }
                Node::Struct(out)
            }
            Shape::Map(entries) => {
                let mut out: Vec<(String, Node)> = Vec::new();
                for (k, s) in entries {
                    if !out.iter().any(|(g, _)| g == k) {
                        out.push((k.clone(), s.into()));
                    }
                }
                Node::Map(out)
            }
            Shape::Boxed(s) => Node::Boxed(Box::new((&**s).into())),
            Shape::Shared(s) => Node::Shared(Arc::new((&**s).into())),
        }
    }
}

/// Hands `value` to the serializer behind `depth` extra references.
fn emit<S: Serializer, U: Serialize>(
    s: S,
    value: &U,
    depth: u8,
    via: Via,
) -> Result<S::Ok, S::Error> {
    fn hand<S: Serializer, T: ?Sized + Serialize>(
        s: S,
        v: &T,
        via: Via,
    ) -> Result<S::Ok, S::Error> {
        match via {
            Via::NewtypeStruct => s.serialize_newtype_struct("Wrap", v),
            Via::Some => s.serialize_some(v),
        }
    }
    match depth {
        0 => hand(s, value, via),
        1 => hand(s, &value, via),
        2 => hand(s, &&value, via),
        _ => hand(s, &&&value, via),
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Node::Null => s.serialize_unit(),
            Node::Bool(b) => s.serialize_bool(*b),
            Node::Int(n) => s.serialize_i64(*n),
            Node::Str(v) => s.serialize_str(v),
            Node::Leaf { leaf, depth, via } => {
                let (d, v) = (*depth, *via);
                match leaf {
                    Leaf::IpAddr(x) => emit(s, x, d, v),
                    Leaf::Ipv4Addr(x) => emit(s, x, d, v),
                    Leaf::Ipv6Addr(x) => emit(s, x, d, v),
                    Leaf::IpNet(x) => emit(s, x, d, v),
                    Leaf::Ipv4Net(x) => emit(s, x, d, v),
                    Leaf::Ipv6Net(x) => emit(s, x, d, v),
                    Leaf::Ip(x) => emit(s, x, d, v),
                    Leaf::Cidr(x) => emit(s, x, d, v),
                    Leaf::Mac(x) => emit(s, x, d, v),
                    Leaf::Url(x) => emit(s, x, d, v),
                    Leaf::UrlCrate(x) => emit(s, x, d, v),
                }
            }
            Node::List(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Node::Struct(fields) => {
                let mut st = s.serialize_struct("Node", fields.len())?;
                for (f, v) in fields {
                    st.serialize_field(f.name(), v)?;
                }
                st.end()
            }
            Node::Map(entries) => {
                let mut map = s.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            Node::Boxed(b) => b.serialize(s),
            Node::Shared(a) => a.serialize(s),
        }
    }
}

/// What a read gives, in a form that compares typed values exactly.
#[derive(Debug, PartialEq)]
pub enum Read {
    Missing,
    /// A struct or map read whole.
    Opaque,
    Value(Value),
}

fn read(val: Option<Val<'_>>) -> Read {
    match val {
        None => Read::Missing,
        Some(Val::Ref(ValueRef::Object(_))) => Read::Opaque,
        Some(val) => Read::Value(val.as_ref().to_owned()),
    }
}

fn leaf_read<U: InputValue + Serialize>(value: &U, depth: u8, rest: &[Segment]) -> Read {
    if depth < 3 {
        return read(InputValue::get(value, rest).expect("leaf read"));
    }
    // Three references: the walk sees serde's text.
    let json = serde_json::to_value(value).expect("serialize leaf");
    let text = json
        .as_str()
        .expect("leaves serialize as strings")
        .to_owned();
    if rest.is_empty() {
        Read::Value(Value::String(text))
    } else {
        Read::Missing
    }
}

/// The model: what `SerdeInput::new(node).get(path)` must return.
pub fn expect(node: &Node, path: &[Segment]) -> Read {
    let Some((first, rest)) = path.split_first() else {
        return whole(node);
    };
    match node {
        Node::Leaf { leaf, depth, .. } => match leaf {
            Leaf::IpAddr(x) => leaf_read(x, *depth, path),
            Leaf::Ipv4Addr(x) => leaf_read(x, *depth, path),
            Leaf::Ipv6Addr(x) => leaf_read(x, *depth, path),
            Leaf::IpNet(x) => leaf_read(x, *depth, path),
            Leaf::Ipv4Net(x) => leaf_read(x, *depth, path),
            Leaf::Ipv6Net(x) => leaf_read(x, *depth, path),
            Leaf::Ip(x) => leaf_read(x, *depth, path),
            Leaf::Cidr(x) => leaf_read(x, *depth, path),
            Leaf::Mac(x) => leaf_read(x, *depth, path),
            Leaf::Url(x) => leaf_read(x, *depth, path),
            Leaf::UrlCrate(x) => leaf_read(x, *depth, path),
        },
        Node::List(items) => match first {
            Segment::Index(i) => items.get(*i).map_or(Read::Missing, |n| expect(n, rest)),
            _ => Read::Missing,
        },
        Node::Struct(fields) => match first {
            Segment::Key { key, .. } => fields
                .iter()
                .find(|(f, _)| f.name() == key)
                .map_or(Read::Missing, |(_, n)| expect(n, rest)),
            _ => Read::Missing,
        },
        Node::Map(entries) => match first {
            Segment::Key { key, .. } => entries
                .iter()
                .find(|(k, _)| k == key)
                .map_or(Read::Missing, |(_, n)| expect(n, rest)),
            _ => Read::Missing,
        },
        Node::Boxed(b) => expect(b, path),
        Node::Shared(a) => expect(a, path),
        Node::Null | Node::Bool(_) | Node::Int(_) | Node::Str(_) => Read::Missing,
    }
}

fn whole(node: &Node) -> Read {
    match node {
        Node::Null => Read::Value(Value::Null),
        Node::Bool(b) => Read::Value(Value::Bool(*b)),
        Node::Int(n) => Read::Value(Value::Int(*n)),
        Node::Str(s) => Read::Value(Value::String(s.as_str().into())),
        Node::Leaf { .. } => expect_leaf_whole(node),
        // An array read whole collects its items; an opaque item stands in
        // as an empty object, a missing one as null.
        Node::List(items) => Read::Value(Value::Array(
            items
                .iter()
                .map(|n| match whole(n) {
                    Read::Value(v) => v,
                    Read::Opaque => Value::Object(Map::default()),
                    Read::Missing => Value::Null,
                })
                .collect(),
        )),
        Node::Struct(_) | Node::Map(_) => Read::Opaque,
        Node::Boxed(b) => whole(b),
        Node::Shared(a) => whole(a),
    }
}

fn expect_leaf_whole(node: &Node) -> Read {
    let Node::Leaf { leaf, depth, .. } = node else {
        unreachable!()
    };
    let d = *depth;
    match leaf {
        Leaf::IpAddr(x) => leaf_read(x, d, &[]),
        Leaf::Ipv4Addr(x) => leaf_read(x, d, &[]),
        Leaf::Ipv6Addr(x) => leaf_read(x, d, &[]),
        Leaf::IpNet(x) => leaf_read(x, d, &[]),
        Leaf::Ipv4Net(x) => leaf_read(x, d, &[]),
        Leaf::Ipv6Net(x) => leaf_read(x, d, &[]),
        Leaf::Ip(x) => leaf_read(x, d, &[]),
        Leaf::Cidr(x) => leaf_read(x, d, &[]),
        Leaf::Mac(x) => leaf_read(x, d, &[]),
        Leaf::Url(x) => leaf_read(x, d, &[]),
        Leaf::UrlCrate(x) => leaf_read(x, d, &[]),
    }
}

fn key(k: &str) -> Segment {
    Segment::Key {
        key: k.to_owned(),
        bracket: false,
    }
}

/// Field names the leaf types answer to, and some they don't.
const LEAF_FIELDS: &[&str] = &[
    "version", "ip", "prefix", "host", "port", "scheme", "path", "query", "user", "zz",
];

/// Every path into `node`, each leaf's built-in fields, and one step past each.
pub fn paths(node: &Node) -> Vec<Vec<Segment>> {
    fn walk(node: &Node, prefix: &mut Vec<Segment>, out: &mut Vec<Vec<Segment>>) {
        out.push(prefix.clone());
        for past in [key("zz_absent"), Segment::Index(0), Segment::Index(999)] {
            let mut p = prefix.clone();
            p.push(past);
            out.push(p);
        }
        match node {
            Node::Leaf { .. } => {
                for f in LEAF_FIELDS {
                    let mut p = prefix.clone();
                    p.push(key(f));
                    out.push(p.clone());
                    p.push(key("zz"));
                    out.push(p);
                }
            }
            Node::List(items) => {
                for (i, n) in items.iter().enumerate() {
                    prefix.push(Segment::Index(i));
                    walk(n, prefix, out);
                    prefix.pop();
                }
            }
            Node::Struct(fields) => {
                for (f, n) in fields {
                    prefix.push(key(f.name()));
                    walk(n, prefix, out);
                    prefix.pop();
                }
            }
            Node::Map(entries) => {
                for (k, n) in entries {
                    prefix.push(key(k));
                    walk(n, prefix, out);
                    prefix.pop();
                }
            }
            Node::Boxed(b) => walk(b, prefix, out),
            Node::Shared(a) => walk(a, prefix, out),
            Node::Null | Node::Bool(_) | Node::Int(_) | Node::Str(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(node, &mut Vec::new(), &mut out);
    out
}

/// `SerdeInput` reads every path of `shape`'s tree as the model does.
pub fn check(shape: &Shape) {
    let node = Node::from(shape);
    let input = SerdeInput::new(&node);
    for path in paths(&node) {
        let got = read(InputValue::get(&input, &path).expect("SerdeInput read"));
        let want = expect(&node, &path);
        assert_eq!(got, want, "path {path:?} in {node:?}");
    }
}

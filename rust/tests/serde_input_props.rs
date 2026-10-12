//! Property tests: random trees of typed leaves through `SerdeInput`,
//! checked against the independent model in `support/serde_tree.rs`.
//! proptest generates and shrinks the trees here; the same model drives the
//! coverage-guided libFuzzer target in `fuzz/` (`make fuzz-rust`).

#[path = "support/serde_tree.rs"]
mod serde_tree;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use proptest::prelude::*;
use serde_tree::{Field, Seed, Shape, Via};

fn seed() -> impl Strategy<Value = Seed> {
    prop_oneof![
        any::<IpAddr>().prop_map(Seed::IpAddr),
        any::<Ipv4Addr>().prop_map(Seed::Ipv4Addr),
        any::<Ipv6Addr>().prop_map(Seed::Ipv6Addr),
        (any::<IpAddr>(), any::<u8>()).prop_map(|(a, p)| Seed::IpNet(a, p)),
        (any::<Ipv4Addr>(), any::<u8>()).prop_map(|(a, p)| Seed::Ipv4Net(a, p)),
        (any::<Ipv6Addr>(), any::<u8>()).prop_map(|(a, p)| Seed::Ipv6Net(a, p)),
        any::<IpAddr>().prop_map(Seed::Ip),
        (any::<IpAddr>(), any::<u8>()).prop_map(|(a, p)| Seed::Cidr(a, p)),
        any::<[u8; 6]>().prop_map(Seed::Mac),
        any::<u8>().prop_map(Seed::Url),
        any::<u8>().prop_map(Seed::UrlCrate),
    ]
}

fn shape() -> impl Strategy<Value = Shape> {
    let via = prop_oneof![Just(Via::NewtypeStruct), Just(Via::Some)];
    let field = prop_oneof![
        Just(Field::A),
        Just(Field::B),
        Just(Field::C),
        Just(Field::D)
    ];
    // Weighted toward leaves: they are what the `unsafe` cast reads.
    let scalar = prop_oneof![
        1 => Just(Shape::Null),
        1 => any::<bool>().prop_map(Shape::Bool),
        1 => any::<i64>().prop_map(Shape::Int),
        1 => "[a-z0-9:/.]{0,8}".prop_map(Shape::Str),
        6 => (seed(), 0u8..4, via).prop_map(|(seed, depth, via)| Shape::Leaf { seed, depth, via }),
    ];
    scalar.prop_recursive(4, 48, 4, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Shape::List),
            prop::collection::vec((field.clone(), inner.clone()), 0..4).prop_map(Shape::Struct),
            prop::collection::vec(("[a-c]{0,2}", inner.clone()), 0..4).prop_map(Shape::Map),
            inner.clone().prop_map(|s| Shape::Boxed(Box::new(s))),
            inner.prop_map(|s| Shape::Shared(Box::new(s))),
        ]
    })
}

fn config() -> ProptestConfig {
    if cfg!(miri) {
        // Miri is ~1000x slower and cannot persist failures to disk; a few
        // dozen trees still reach every leaf type at every depth.
        ProptestConfig {
            cases: 32,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    } else {
        ProptestConfig::with_cases(2000)
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn random_trees_read_as_the_model_says(shape in shape()) {
        serde_tree::check(&shape);
    }
}

/// Every leaf type at every reference depth, through both hand-offs and
/// inside each container, regardless of what the random cases cover.
#[test]
fn every_leaf_at_every_depth() {
    let seeds = [
        Seed::IpAddr("10.1.2.3".parse().unwrap()),
        Seed::IpAddr("::ffff:10.1.2.3".parse().unwrap()),
        Seed::Ipv4Addr("192.168.1.1".parse().unwrap()),
        Seed::Ipv6Addr("fe80::1".parse().unwrap()),
        Seed::IpNet("10.0.0.0".parse().unwrap(), 8),
        Seed::Ipv4Net("172.16.0.0".parse().unwrap(), 32),
        Seed::Ipv6Net("2001:db8::".parse().unwrap(), 128),
        Seed::Ip("::1".parse().unwrap()),
        Seed::Cidr("fd00::".parse().unwrap(), 0),
        Seed::Mac([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]),
        Seed::Url(0),
        Seed::Url(1),
        Seed::Url(2),
        Seed::UrlCrate(1),
        Seed::UrlCrate(4),
    ];
    for seed in seeds {
        for depth in 0..4 {
            for via in [Via::NewtypeStruct, Via::Some] {
                let leaf = Shape::Leaf {
                    seed: seed.clone(),
                    depth,
                    via,
                };
                serde_tree::check(&leaf);
                serde_tree::check(&Shape::List(vec![leaf.clone(), Shape::Null]));
                serde_tree::check(&Shape::Struct(vec![(
                    Field::A,
                    Shape::Boxed(Box::new(leaf.clone())),
                )]));
                serde_tree::check(&Shape::Map(vec![(
                    "k".into(),
                    Shape::Shared(Box::new(leaf)),
                )]));
            }
        }
    }
}

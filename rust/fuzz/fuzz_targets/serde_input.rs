//! `SerdeInput` over arbitrary trees of typed leaves behind zero to three
//! references, checked against the model in `tests/support/serde_tree.rs`.
//! This is the coverage-guided counterpart of `tests/serde_input_props.rs`.

#![no_main]

#[path = "../../tests/support/serde_tree.rs"]
mod serde_tree;

libfuzzer_sys::fuzz_target!(|shape: serde_tree::Shape| serde_tree::check(&shape));

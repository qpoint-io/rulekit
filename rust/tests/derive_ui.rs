//! `#[derive(Args)]` rejects unsupported shapes with clear errors.

#[test]
fn derive_errors() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}

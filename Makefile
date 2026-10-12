.PHONY: test ci ci-go ci-rust miri-rust fuzz-rust

test:
	go test ./...

# CI runs ci-go and ci-rust as separate jobs (.github/workflows/ci.yml).
ci: ci-go ci-rust

ci-go:
	go test ./...

ci-rust:
	cd rust && cargo fmt --all --check
	cd rust && cargo clippy --workspace --all-targets --locked -- -D warnings
	cd rust && cargo test --workspace --locked

# The `unsafe` downcast in rust/src/serde_input.rs: its unit tests and the
# serde_input suites under Miri, with both aliasing models, strict
# provenance, and symbolic alignment (nightly + miri). The unit tests also
# run without default features, where `url::Url` is not a leaf.
MIRI_CHECKS := -Zmiri-strict-provenance -Zmiri-symbolic-alignment-check
miri-rust:
	cd rust && MIRIFLAGS="$(MIRI_CHECKS)" cargo +nightly miri test --lib downcast_tests
	cd rust && MIRIFLAGS="$(MIRI_CHECKS)" cargo +nightly miri test --lib --no-default-features downcast_tests
	cd rust && MIRIFLAGS="$(MIRI_CHECKS)" cargo +nightly miri test --test serde_input --test serde_input_props
	cd rust && MIRIFLAGS="$(MIRI_CHECKS) -Zmiri-tree-borrows" cargo +nightly miri test --lib downcast_tests
	cd rust && MIRIFLAGS="$(MIRI_CHECKS) -Zmiri-tree-borrows" cargo +nightly miri test --test serde_input --test serde_input_props

# Coverage-guided fuzzing of SerdeInput (nightly + cargo-fuzz).
# FUZZ_SECONDS=0 runs until stopped.
FUZZ_SECONDS ?= 60
fuzz-rust:
	cd rust && cargo +nightly fuzz run serde_input -- -max_total_time=$(FUZZ_SECONDS) -max_len=4096

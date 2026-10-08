.PHONY: test ci ci-go ci-rust

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

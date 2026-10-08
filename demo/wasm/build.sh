#!/usr/bin/env bash
# Build the Rust WASM bridge into src/wasm/ (an ES module, its .d.ts, and
# rulekit_bg.wasm), which src/lib/rulekit.ts imports.
set -euo pipefail

cd "$(dirname "$0")"

want=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' Cargo.toml)
have=$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2 || true)
if [[ "$have" != "$want" ]]; then
  echo "wasm/build.sh: needs wasm-bindgen CLI $want (found: ${have:-none}). Install it with:" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown --target-dir target
wasm-bindgen --target web --out-dir ../src/wasm --out-name rulekit \
  target/wasm32-unknown-unknown/release/rulekit_demo_wasm.wasm

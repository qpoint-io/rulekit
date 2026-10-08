#!/bin/sh
# Compiled size of the traced evaluation (D7): release builds of
# examples/size_probe with and without the traced instantiation, plus the
# rlib size.
set -eu
cd "$(dirname "$0")/.."
size() { stat -f %z "$1" 2>/dev/null || stat -c %s "$1"; }
build() { cargo build -q --release --example size_probe --target-dir "target/size-$1" ${2:+--config "build.rustflags=['--cfg','$2']"}; }
build full
build probe rulekit_size_probe
full=target/size-full/release/examples/size_probe
probe=target/size-probe/release/examples/size_probe
strip -o "$full.stripped" "$full" 2>/dev/null || cp "$full" "$full.stripped"
strip -o "$probe.stripped" "$probe" 2>/dev/null || cp "$probe" "$probe.stripped"
echo "size_probe, with traced eval:    $(size "$full.stripped") bytes (stripped)"
echo "size_probe, without traced eval: $(size "$probe.stripped") bytes (stripped)"
echo "traced eval instantiation:       $(( $(size "$full.stripped") - $(size "$probe.stripped") )) bytes"
echo "librulekit.rlib (release):       $(size "$(ls target/size-full/release/deps/librulekit-*.rlib | head -1)") bytes"

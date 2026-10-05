#!/bin/sh
# Run the Go and Rust benchmarks and print them side by side (ns/op and
# allocs/op). Usage: rust/bench/compare.sh [criterion filter]
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

(cd "$root" && go test -run '^$' -bench 'BenchmarkEval$|BenchmarkEvalLazyInput|BenchmarkEvalTrace|BenchmarkParse|BenchmarkCmpNumber' \
    -benchmem -count 1 . > "$out/go.txt")
(cd "$root/rust" && cargo bench -q --bench eval -- --noplot ${1:-} > /dev/null 2>&1 \
    && cargo bench -q --bench allocs > "$out/allocs.txt" 2>/dev/null)

python3 - "$out" "$root/rust/target/criterion" <<'PY'
import json, os, re, sys
out, crit = sys.argv[1], sys.argv[2]
go = {}
for line in open(os.path.join(out, "go.txt")):
    m = re.match(r"(Benchmark\S+?)(-\d+)?\s+\d+\s+([\d.]+) ns/op(?:\s+\d+ B/op\s+(\d+) allocs/op)?", line)
    if m:
        go[m.group(1)] = (float(m.group(3)), m.group(4) or "-")
allocs = {}
for line in open(os.path.join(out, "allocs.txt")):
    name, n, _ = line.split()
    allocs[name] = n
rust = {}
for dirpath, _, files in os.walk(crit):
    if "estimates.json" in files and dirpath.endswith(os.path.join("new")):
        name = os.path.relpath(os.path.dirname(dirpath), crit)
        est = json.load(open(os.path.join(dirpath, "estimates.json")))
        rust[name] = est["median"]["point_estimate"]
print(f"{'benchmark':58} {'go ns':>9} {'rust ns':>9} {'ratio':>6} {'go allocs':>9} {'rust allocs':>11}")
for name in sorted(set(go) | set(rust)):
    if name not in rust or name not in go:
        continue
    g, ga = go[name]
    r = rust[name]
    print(f"{name:58} {g:9.1f} {r:9.1f} {r / g:6.2f} {ga:>9} {allocs.get(name, '-'):>11}")
PY

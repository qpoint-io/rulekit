//! Timing benchmarks named after the Go benchmarks (`go test -bench`).

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use rulekit::Opts;

mod cases;

fn bench_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("BenchmarkEval");
    for case in cases::eval_cases() {
        group.bench_function(case.name, |b| b.iter(|| black_box(case.eval())));
    }
    group.finish();

    let mut group = c.benchmark_group("BenchmarkEvalLazyInput");
    for (name, rule, warm) in cases::lazy_cases() {
        let input = cases::lazy_input();
        if warm {
            rule.eval(&(), &input, Opts::default());
        }
        group.bench_function(name, |b| {
            b.iter(|| black_box(rule.eval(&(), &input, Opts::default()).pass()))
        });
    }
    let rule = rulekit::parse(r#"expensive == "value""#).unwrap();
    group.bench_function("resolved_per_eval", |b| {
        b.iter(|| {
            let input = cases::lazy_input();
            black_box(rule.eval(&(), &input, Opts::default()).pass())
        })
    });
    group.finish();

    let trace = cases::trace_case();
    c.bench_function("BenchmarkEvalTrace", |b| b.iter(|| black_box(trace.eval())));

    let derived = cases::DerivedBench::new();
    c.bench_function("BenchmarkEval/derived_struct", |b| {
        b.iter(|| black_box(derived.eval()))
    });
    c.bench_function("BenchmarkEval/derived_vs_kv", |b| {
        b.iter(|| black_box(derived.kv.eval()))
    });

    let mut group = c.benchmark_group("BenchmarkParse");
    for (name, expr) in cases::PARSE_CASES {
        group.bench_function(name, |b| {
            b.iter(|| black_box(rulekit::parse(black_box(expr)).is_ok()))
        });
    }
    group.finish();

    let mut group = c.benchmark_group("BenchmarkCmpNumber");
    for (xn, x) in cases::cmp_values() {
        for (yn, y) in cases::cmp_values() {
            group.bench_function(format!("{xn}-{yn}"), |b| {
                b.iter(|| black_box(rulekit::__bench::cmp_number(black_box(x), black_box(y))))
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_eval);
criterion_main!(benches);

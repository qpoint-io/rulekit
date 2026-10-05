//! Allocations per operation for the benchmark cases (criterion does not
//! count allocations). Prints `name allocs/op bytes/op`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use rulekit::Opts;

mod cases;

struct Counting;

thread_local! {
    static COUNT: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| {
            let (n, b) = c.get();
            c.set((n + 1, b + layout.size() as u64));
        });
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const N: u64 = 1000;

fn measure(name: &str, mut f: impl FnMut()) {
    f();
    let (n0, b0) = COUNT.with(Cell::get);
    for _ in 0..N {
        f();
    }
    let (n1, b1) = COUNT.with(Cell::get);
    println!("{name} {} {}", (n1 - n0) / N, (b1 - b0) / N);
}

fn main() {
    for case in cases::eval_cases() {
        measure(&format!("BenchmarkEval/{}", case.name), || {
            black_box(case.eval());
        });
    }
    for (name, rule, warm) in cases::lazy_cases() {
        let input = cases::lazy_input();
        if warm {
            rule.eval(&input, &(), Opts::default());
        }
        measure(&format!("BenchmarkEvalLazyInput/{name}"), || {
            black_box(rule.eval(&input, &(), Opts::default()).pass());
        });
    }
    let rule = rulekit::parse(r#"expensive == "value""#).unwrap();
    measure("BenchmarkEvalLazyInput/resolved_per_eval", || {
        let input = cases::lazy_input();
        black_box(rule.eval(&input, &(), Opts::default()).pass());
    });
    let trace = cases::trace_case();
    measure("BenchmarkEvalTrace", || {
        black_box(trace.eval());
    });
    for (name, expr) in cases::PARSE_CASES {
        measure(&format!("BenchmarkParse/{name}"), || {
            black_box(rulekit::parse(black_box(expr)).is_ok());
        });
    }
}

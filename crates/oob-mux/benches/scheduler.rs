//! Criterion bench of the class scheduler under saturation (issue #16,
//! ADR-002 action item 4). It sets the number `PacerConfig::pull_budget`
//! documents: what one `Pacer::pull(64)` costs with every class full.
//!
//! Fixture values only: real budgets are runtime configuration.

#[path = "../tests/common/mod.rs"]
mod common;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use oob_mux::{PullSource, TrafficClass};

fn saturated_pull(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduler_saturated");
    for pull_n in [16usize, 64] {
        group.throughput(Throughput::Elements(pull_n as u64));
        group.bench_function(format!("pull_{pull_n}_six_classes_full"), |b| {
            let mut r = common::rig(common::roomy(true));
            let mut out = Vec::with_capacity(64);
            b.iter(|| {
                // The producers keep every class at depth; the refill is
                // part of the measured cost, as it is in the daemon.
                for class in TrafficClass::ALL {
                    common::saturate(&r.handle, class);
                }
                out.clear();
                let report = r.pacer.pull(pull_n, &mut out).unwrap();
                std::hint::black_box(report);
            });
        });
    }
    group.finish();
}

/// The design note's shape: six classes with 1000 PDUs queued each.
fn deep_queues(c: &mut Criterion) {
    c.bench_function("scheduler_saturated/pull_64_1000_queued_per_class", |b| {
        let mut r = common::rig_depth(common::roomy(true), 1000);
        for class in TrafficClass::ALL {
            common::saturate(&r.handle, class);
        }
        let mut out = Vec::with_capacity(64);
        b.iter(|| {
            for class in TrafficClass::ALL {
                common::saturate(&r.handle, class);
            }
            out.clear();
            std::hint::black_box(r.pacer.pull(64, &mut out).unwrap());
        });
    });
}

/// The same pull with the queues full of work but only the pick measured:
/// the producers refill outside the timed section.
fn pick_only(c: &mut Criterion) {
    c.bench_function("scheduler_saturated/pull_64_pick_only", |b| {
        let mut r = common::rig(common::roomy(true));
        let mut out = Vec::with_capacity(64);
        b.iter_batched(
            || {
                for class in TrafficClass::ALL {
                    common::saturate(&r.handle, class);
                }
            },
            |()| {
                out.clear();
                std::hint::black_box(r.pacer.pull(64, &mut out).unwrap());
            },
            criterion::BatchSize::PerIteration,
        );
    });
}

criterion_group!(benches, saturated_pull, deep_queues, pick_only);
criterion_main!(benches);

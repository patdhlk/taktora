//! Seqlock ring semantics: FIFO when the consumer keeps up, lap-counted loss
//! when it falls behind, and tear-free reads under a concurrent producer.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use taktora_telemetry_export::{CycleRing, PodRecord, RecvOutcome};

// A record whose fields encode an invariant a torn read would violate:
// period_ns == cycle_index * 7 + 3 and took_ns == cycle_index. A consumer
// that reads two writes spliced together fails this check.
const fn marked(cycle_index: u64) -> PodRecord {
    PodRecord::new_healthy(
        cycle_index,
        0,
        cycle_index,
        cycle_index * 7 + 3,
        0,
        0,
        0,
        cycle_index,
    )
}

fn drain_all(consumer: &mut taktora_telemetry_export::Consumer) -> (Vec<u64>, u64) {
    let mut got = Vec::new();
    let mut lapped = 0;
    loop {
        match consumer.try_recv() {
            RecvOutcome::Record(r) => got.push(r.cycle_index),
            RecvOutcome::Lapped { skipped } => lapped += skipped,
            RecvOutcome::Empty => break,
        }
    }
    (got, lapped)
}

#[test]
fn fifo_when_consumer_keeps_up() {
    let (producer, mut consumer) = CycleRing::with_capacity(8).split();
    for i in 0..6 {
        producer.push(marked(i));
    }
    let (got, lapped) = drain_all(&mut consumer);
    assert_eq!(got, vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(lapped, 0);
    assert!(matches!(consumer.try_recv(), RecvOutcome::Empty));
}

#[test]
fn overwrite_oldest_counts_lapped() {
    // Capacity 4; push 10 without draining. The 4 newest survive (6..=9),
    // the oldest 6 (0..=5) were overwritten and must be counted as lapped.
    let (producer, mut consumer) = CycleRing::with_capacity(4).split();
    for i in 0..10 {
        producer.push(marked(i));
    }
    let (got, lapped) = drain_all(&mut consumer);
    assert_eq!(lapped, 6, "0..=5 were lapped");
    assert_eq!(got, vec![6, 7, 8, 9], "only the 4 newest survive");
}

#[test]
fn concurrent_producer_is_tear_free_and_lossless_in_total() {
    // One producer pushes N marked records as fast as it can; one consumer
    // drains concurrently. Every record the consumer *sees* must satisfy the
    // tear invariant, and (received + lapped) must equal N exactly.
    const N: u64 = 200_000;
    let (producer, mut consumer) = CycleRing::with_capacity(1024).split();
    let done = Arc::new(AtomicBool::new(false));
    let done_w = done.clone();

    let prod = thread::spawn(move || {
        for i in 0..N {
            producer.push(marked(i));
        }
        done_w.store(true, Ordering::Release);
    });

    let mut received = 0u64;
    let mut lapped = 0u64;
    loop {
        match consumer.try_recv() {
            RecvOutcome::Record(r) => {
                assert_eq!(
                    r.period_ns,
                    r.cycle_index * 7 + 3,
                    "torn read detected: period_ns inconsistent with cycle_index"
                );
                assert_eq!(r.took_ns, r.cycle_index, "torn read detected: took_ns");
                received += 1;
            }
            RecvOutcome::Lapped { skipped } => lapped += skipped,
            RecvOutcome::Empty => {
                if done.load(Ordering::Acquire) {
                    match consumer.try_recv() {
                        RecvOutcome::Empty => break,
                        RecvOutcome::Record(r) => {
                            assert_eq!(r.period_ns, r.cycle_index * 7 + 3);
                            received += 1;
                        }
                        RecvOutcome::Lapped { skipped } => lapped += skipped,
                    }
                }
            }
        }
    }
    prod.join().unwrap();
    assert_eq!(
        received + lapped,
        N,
        "every record is either seen or counted as lapped"
    );
}

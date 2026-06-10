//! O(log n) attachment-id → task-index resolution for the dispatch hot path
//! (issue #94, `ADR_0106`). A single sorted `Vec` resolved by `binary_search`,
//! with lazy-learn + negative caching for ids that cannot be precomputed
//! (deadline real-event Notification-form ids) or that belong to no task
//! (master timer / stop listener). Capacity is reserved up front so
//! steady-state resolution is allocation-free (`REQ_0060`).

// pub(crate) inside a private module — intentional, mirrors `executor.rs`; the
// dispatch loop reaches for these items from `executor` (#94).
#![allow(clippy::redundant_pub_crate)]

use iceoryx2::prelude::ipc;
use iceoryx2::prelude::{WaitSetAttachmentId, WaitSetGuard};

/// Sentinel task index for ids that resolve to no task (negative cache).
pub(crate) const IGNORE: usize = usize::MAX;

type Id = WaitSetAttachmentId<ipc::Service>;

pub(crate) struct AttachmentMap {
    /// Sorted by id; `binary_search` on lookup, `insert` at `Err(pos)` on learn.
    entries: Vec<(Id, usize)>,
}

impl AttachmentMap {
    /// Build the precomputed entries from each guard's `from_guard` id.
    /// `deadline_count` reserves room for the lazy-learned Notification-form
    /// ids (one per deadline attachment); `+ 2` reserves the master-timer and
    /// stop-listener negative-cache entries — the only ids that fire from
    /// outside `guards`.
    pub(crate) fn build(
        guards: &[WaitSetGuard<'_, '_, ipc::Service>],
        attachment_to_task: &[usize],
        deadline_count: usize,
    ) -> Self {
        let mut entries: Vec<(Id, usize)> = Vec::with_capacity(guards.len() + deadline_count + 2);
        for (i, guard) in guards.iter().enumerate() {
            entries.push((
                WaitSetAttachmentId::from_guard(guard),
                attachment_to_task[i],
            ));
        }
        // Unique keys (one fd per attachment, unique tick indices), so an
        // unstable sort is correct and matches the alloc-conscious style.
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        Self { entries }
    }

    /// Resolve a fired id. Hit → cached value (slow NOT called). Miss → run
    /// `slow` once, cache the result (positive or `IGNORE`), return it.
    pub(crate) fn resolve<F: FnMut(&Id) -> usize>(&mut self, id: &Id, mut slow: F) -> usize {
        match self.entries.binary_search_by(|(k, _)| k.cmp(id)) {
            Ok(pos) => self.entries[pos].1,
            Err(pos) => {
                let v = slow(id);
                // The invariant that actually defends REQ_0060: a learn must
                // never realloc in steady state. If the "deadline_count + 2
                // unknown ids" inventory in `build` is ever stale (a future
                // attachment held outside `guards`, a third out-of-band wake
                // source), this fires instead of a silent steady-state realloc
                // that no allocation test would catch.
                debug_assert!(
                    self.entries.len() < self.entries.capacity(),
                    "AttachmentMap learn exceeded reserved capacity — unknown-id \
                     inventory is stale (REQ_0060)"
                );
                self.entries.insert(pos, (id.clone(), v));
                v
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iceoryx2::prelude::{WaitSet, WaitSetBuilder};
    use std::cell::Cell;
    use std::time::Duration;

    /// Spin up a throwaway `WaitSet` plus `n` distinct interval guards, each
    /// carrying a unique `Tick`-form id. Interval guards need no listener, so
    /// this is the cheapest real source of `WaitSetAttachmentId`s for tests.
    fn make_guards(
        waitset: &WaitSet<ipc::Service>,
        n: usize,
    ) -> Vec<WaitSetGuard<'_, '_, ipc::Service>> {
        (0..n)
            .map(|i| {
                // Distinct durations → distinct interval attachments → distinct ids.
                waitset
                    .attach_interval(Duration::from_millis((i as u64) + 1))
                    .expect("attach_interval")
            })
            .collect()
    }

    /// Arbitrary distinct task indices: 10, 20, 30, …
    fn task_indices(n: usize) -> Vec<usize> {
        (0..n).map(|i| (i + 1) * 10).collect()
    }

    #[test]
    fn precomputed_ids_round_trip_without_slow_path() {
        let waitset = WaitSetBuilder::new().create().expect("create waitset");
        let guards = make_guards(&waitset, 5);
        let tasks = task_indices(guards.len());

        let mut map = AttachmentMap::build(&guards, &tasks, 0);

        let slow_calls = Cell::new(0_usize);
        for (i, guard) in guards.iter().enumerate() {
            let id = WaitSetAttachmentId::from_guard(guard);
            let got = map.resolve(&id, |_| {
                slow_calls.set(slow_calls.get() + 1);
                IGNORE
            });
            assert_eq!(got, tasks[i], "precomputed id {i} resolved wrong task");
        }
        assert_eq!(slow_calls.get(), 0, "slow path ran for a precomputed id");
    }

    #[test]
    fn unknown_id_lazy_learns_exactly_once_then_fast() {
        let waitset = WaitSetBuilder::new().create().expect("create waitset");
        let guards = make_guards(&waitset, 4);
        let tasks = task_indices(guards.len());

        // Hold out the last guard — it is NOT passed to `build`, so its id is
        // an unknown that must be lazy-learned. Reserve room for it via
        // `deadline_count = 1`.
        let (built_guards, held) = guards.split_at(guards.len() - 1);
        let held_id = WaitSetAttachmentId::from_guard(&held[0]);
        let held_task = tasks[tasks.len() - 1];

        let mut map = AttachmentMap::build(built_guards, &tasks[..built_guards.len()], 1);

        let slow_calls = Cell::new(0_usize);
        let resolve_held = |map: &mut AttachmentMap| {
            map.resolve(&held_id, |_| {
                slow_calls.set(slow_calls.get() + 1);
                held_task
            })
        };

        assert_eq!(slow_calls.get(), 0);
        let first = resolve_held(&mut map);
        assert_eq!(first, held_task);
        assert_eq!(slow_calls.get(), 1, "first miss must run slow once");

        let second = resolve_held(&mut map);
        assert_eq!(second, held_task);
        assert_eq!(slow_calls.get(), 1, "second resolve must hit the cache");
    }

    #[test]
    fn no_match_id_caches_ignore() {
        let waitset = WaitSetBuilder::new().create().expect("create waitset");
        let guards = make_guards(&waitset, 3);
        let tasks = task_indices(guards.len());

        let (built_guards, held) = guards.split_at(guards.len() - 1);
        let held_id = WaitSetAttachmentId::from_guard(&held[0]);

        let mut map = AttachmentMap::build(built_guards, &tasks[..built_guards.len()], 1);

        let slow_calls = Cell::new(0_usize);
        let resolve_held = |map: &mut AttachmentMap| {
            map.resolve(&held_id, |_| {
                slow_calls.set(slow_calls.get() + 1);
                IGNORE
            })
        };

        assert_eq!(slow_calls.get(), 0);
        let first = resolve_held(&mut map);
        assert_eq!(first, IGNORE);
        assert_eq!(slow_calls.get(), 1, "first miss must run slow once");

        let second = resolve_held(&mut map);
        assert_eq!(second, IGNORE, "negative cache must persist");
        assert_eq!(
            slow_calls.get(),
            1,
            "second resolve must hit the negative cache"
        );
    }

    #[test]
    #[should_panic(expected = "exceeded reserved capacity")]
    fn learn_past_reserved_capacity_trips_debug_assert() {
        // build([], [], 0) reserves exactly `0 + 0 + 2 = 2` slots. Three
        // distinct unknown ids → the third insert would push len 2 → 3 past the
        // reserved capacity, tripping the REQ_0060 debug_assert.
        let waitset = WaitSetBuilder::new().create().expect("create waitset");
        let guards = make_guards(&waitset, 3);

        let mut map = AttachmentMap::build(&[], &[], 0);

        for guard in &guards {
            let id = WaitSetAttachmentId::from_guard(guard);
            map.resolve(&id, |_| 0);
        }
    }
}

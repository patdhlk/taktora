//! TSR_0002 — partitioned quota isolation tests.
//!
//! Proves that exhaustion of the quality-managed pool does not deny
//! allocations from the safety-critical pool, and vice versa.

#![allow(unsafe_code)]
#![allow(clippy::doc_markdown)]

use core::alloc::{GlobalAlloc, Layout};
use taktora_bounded_alloc::{
    IntegrityLevel, PartitionedBoundedAllocator, partitioned_bounded_allocator,
};

// Small pools for easy exhaustion testing.
static ALLOC: PartitionedBoundedAllocator<4, 4, 64, 1, 1> =
    partitioned_bounded_allocator!(4, 4, 64);

#[test]
fn qm_exhaustion_does_not_block_sc() {
    let layout = Layout::from_size_align(32, 8).unwrap();
    let mut qm_pointers = [core::ptr::null_mut::<u8>(); 4];

    // Exhaust the QM pool.
    for slot in &mut qm_pointers {
        // SAFETY: alloc_in returns a pointer to an exclusive block.
        let p = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
        assert!(!p.is_null(), "QM pool first four allocations must succeed");
        *slot = p;
    }

    // Fifth QM allocation must fail-closed (pool exhausted).
    let qm_overflow = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(
        qm_overflow.is_null(),
        "QM pool 5th allocation must return null (cap = 4)"
    );

    // SC pool must still have all 4 blocks free — QM exhaustion does not affect SC.
    let mut sc_pointers = [core::ptr::null_mut::<u8>(); 4];
    for slot in &mut sc_pointers {
        let p = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
        assert!(
            !p.is_null(),
            "SC pool allocations must succeed despite QM exhaustion (TSR_0002)"
        );
        *slot = p;
    }

    // Now SC pool is also exhausted.
    let sc_overflow = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(
        sc_overflow.is_null(),
        "SC pool 5th allocation must return null (cap = 4)"
    );

    // Cleanup QM pool.
    for p in qm_pointers {
        unsafe { ALLOC.dealloc_in(IntegrityLevel::QualityManaged, p, layout) };
    }

    // Cleanup SC pool.
    for p in sc_pointers {
        unsafe { ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, p, layout) };
    }
}

#[test]
fn sc_exhaustion_does_not_block_qm() {
    let layout = Layout::from_size_align(32, 8).unwrap();
    let mut sc_pointers = [core::ptr::null_mut::<u8>(); 4];

    // Exhaust the SC pool.
    for slot in &mut sc_pointers {
        let p = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
        assert!(!p.is_null(), "SC pool first four allocations must succeed");
        *slot = p;
    }

    // Fifth SC allocation must fail-closed (pool exhausted).
    let sc_overflow = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(
        sc_overflow.is_null(),
        "SC pool 5th allocation must return null (cap = 4)"
    );

    // QM pool must still have all 4 blocks free — SC exhaustion does not affect QM.
    let mut qm_pointers = [core::ptr::null_mut::<u8>(); 4];
    for slot in &mut qm_pointers {
        let p = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
        assert!(
            !p.is_null(),
            "QM pool allocations must succeed despite SC exhaustion (TSR_0002)"
        );
        *slot = p;
    }

    // Now QM pool is also exhausted.
    let qm_overflow = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(
        qm_overflow.is_null(),
        "QM pool 5th allocation must return null (cap = 4)"
    );

    // Cleanup SC pool.
    for p in sc_pointers {
        unsafe { ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, p, layout) };
    }

    // Cleanup QM pool.
    for p in qm_pointers {
        unsafe { ALLOC.dealloc_in(IntegrityLevel::QualityManaged, p, layout) };
    }
}

#[test]
fn dealloc_in_recovers_pool_capacity() {
    let layout = Layout::from_size_align(32, 8).unwrap();

    // Allocate from SC pool.
    let sc1 = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(!sc1.is_null());
    let sc2 = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(!sc2.is_null());

    // Free one SC block.
    unsafe { ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, sc1, layout) };

    // Re-allocate — must succeed.
    let sc_realloc = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(!sc_realloc.is_null(), "freed SC block must be reusable");

    // Allocate from QM pool.
    let qm1 = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(!qm1.is_null());
    let qm2 = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(!qm2.is_null());

    // Free one QM block.
    unsafe { ALLOC.dealloc_in(IntegrityLevel::QualityManaged, qm1, layout) };

    // Re-allocate — must succeed.
    let qm_realloc = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(!qm_realloc.is_null(), "freed QM block must be reusable");

    // Cleanup.
    unsafe {
        ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, sc2, layout);
        ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, sc_realloc, layout);
        ALLOC.dealloc_in(IntegrityLevel::QualityManaged, qm2, layout);
        ALLOC.dealloc_in(IntegrityLevel::QualityManaged, qm_realloc, layout);
    }
}

#[test]
fn dealloc_auto_detects_pool() {
    let layout = Layout::from_size_align(32, 8).unwrap();

    // Allocate from both pools.
    let sc_ptr = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(!sc_ptr.is_null());
    let qm_ptr = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(!qm_ptr.is_null());

    // Deallocate via the auto-detecting `dealloc` method (no level argument).
    unsafe {
        ALLOC.dealloc(sc_ptr, layout);
        ALLOC.dealloc(qm_ptr, layout);
    }

    // Both should be reusable now.
    let sc_realloc = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(
        !sc_realloc.is_null(),
        "auto-detect dealloc must have freed the SC block"
    );
    let qm_realloc = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(
        !qm_realloc.is_null(),
        "auto-detect dealloc must have freed the QM block"
    );

    // Cleanup.
    unsafe {
        ALLOC.dealloc(sc_realloc, layout);
        ALLOC.dealloc(qm_realloc, layout);
    }
}

#[test]
fn oversize_fails_both_pools() {
    let big = Layout::from_size_align(128, 8).unwrap();
    let sc_big = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, big) };
    assert!(
        sc_big.is_null(),
        "size > BLOCK_SIZE must return null for SC pool"
    );
    let qm_big = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, big) };
    assert!(
        qm_big.is_null(),
        "size > BLOCK_SIZE must return null for QM pool"
    );
}

#[test]
fn excessive_alignment_fails_both_pools() {
    let aligned = Layout::from_size_align(8, 128).unwrap();
    let sc_aligned = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, aligned) };
    assert!(
        sc_aligned.is_null(),
        "align > 64 must return null for SC pool"
    );
    let qm_aligned = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, aligned) };
    assert!(
        qm_aligned.is_null(),
        "align > 64 must return null for QM pool"
    );
}

#[test]
fn counter_independence() {
    let layout = Layout::from_size_align(32, 8).unwrap();

    let sc_allocs_before = ALLOC.sc_alloc_count();
    let qm_allocs_before = ALLOC.qm_alloc_count();

    // Allocate from SC pool.
    let sc_ptr = unsafe { ALLOC.alloc_in(IntegrityLevel::SafetyCritical, layout) };
    assert!(!sc_ptr.is_null());

    // Only SC counter should increment.
    assert_eq!(ALLOC.sc_alloc_count(), sc_allocs_before + 1);
    assert_eq!(ALLOC.qm_alloc_count(), qm_allocs_before);

    // Allocate from QM pool.
    let qm_ptr = unsafe { ALLOC.alloc_in(IntegrityLevel::QualityManaged, layout) };
    assert!(!qm_ptr.is_null());

    // Only QM counter should increment.
    assert_eq!(ALLOC.qm_alloc_count(), qm_allocs_before + 1);
    assert_eq!(ALLOC.sc_alloc_count(), sc_allocs_before + 1);

    // Cleanup.
    unsafe {
        ALLOC.dealloc_in(IntegrityLevel::SafetyCritical, sc_ptr, layout);
        ALLOC.dealloc_in(IntegrityLevel::QualityManaged, qm_ptr, layout);
    }

    assert_eq!(ALLOC.sc_dealloc_count(), 1);
    assert_eq!(ALLOC.qm_dealloc_count(), 1);
}

#[test]
#[cfg(feature = "std")]
fn global_alloc_defaults_to_qm() {
    let layout = Layout::from_size_align(32, 8).unwrap();

    let qm_allocs_before = ALLOC.qm_alloc_count();
    let sc_allocs_before = ALLOC.sc_alloc_count();

    // GlobalAlloc::alloc should route to QM pool when thread-local is not set.
    let ptr = unsafe {
        <PartitionedBoundedAllocator<4, 4, 64, 1, 1> as GlobalAlloc>::alloc(&ALLOC, layout)
    };
    assert!(!ptr.is_null());

    // Only QM counter should increment.
    assert_eq!(ALLOC.qm_alloc_count(), qm_allocs_before + 1);
    assert_eq!(ALLOC.sc_alloc_count(), sc_allocs_before);

    // Cleanup.
    unsafe {
        <PartitionedBoundedAllocator<4, 4, 64, 1, 1> as GlobalAlloc>::dealloc(&ALLOC, ptr, layout);
    }
}

#[test]
#[cfg(feature = "std")]
fn thread_local_routing() {
    use taktora_bounded_alloc::set_current_integrity_level;

    let layout = Layout::from_size_align(32, 8).unwrap();

    // Set thread-local to SafetyCritical.
    set_current_integrity_level(IntegrityLevel::SafetyCritical);

    let sc_allocs_before = ALLOC.sc_alloc_count();
    let qm_allocs_before = ALLOC.qm_alloc_count();

    // GlobalAlloc::alloc should now route to SC pool.
    let ptr = unsafe {
        <PartitionedBoundedAllocator<4, 4, 64, 1, 1> as GlobalAlloc>::alloc(&ALLOC, layout)
    };
    assert!(!ptr.is_null());

    // Only SC counter should increment.
    assert_eq!(ALLOC.sc_alloc_count(), sc_allocs_before + 1);
    assert_eq!(ALLOC.qm_alloc_count(), qm_allocs_before);

    // Cleanup and reset.
    unsafe {
        <PartitionedBoundedAllocator<4, 4, 64, 1, 1> as GlobalAlloc>::dealloc(&ALLOC, ptr, layout);
    }
    set_current_integrity_level(IntegrityLevel::QualityManaged);
}

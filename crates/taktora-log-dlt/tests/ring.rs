use taktora_log_dlt::ring::OfflineRing;

#[test]
fn fifo_when_not_full() {
    let ring = OfflineRing::with_capacity(3);
    ring.push(b"a".to_vec());
    ring.push(b"b".to_vec());
    let drained = ring.drain_all();
    assert_eq!(drained, vec![b"a".to_vec(), b"b".to_vec()]);
    assert_eq!(ring.drops_since_last_drain(), 0);
}

#[test]
fn drop_oldest_when_full() {
    let ring = OfflineRing::with_capacity(2);
    ring.push(b"a".to_vec());
    ring.push(b"b".to_vec());
    ring.push(b"c".to_vec());
    // 'a' dropped; capacity-2 ring keeps 'b' and 'c'.
    assert_eq!(ring.drops_since_last_drain(), 1);
    let drained = ring.drain_all();
    assert_eq!(drained, vec![b"b".to_vec(), b"c".to_vec()]);
}

#[test]
fn drop_counter_resets_after_drain() {
    let ring = OfflineRing::with_capacity(1);
    ring.push(b"a".to_vec());
    ring.push(b"b".to_vec()); // drops a
    ring.push(b"c".to_vec()); // drops b
    assert_eq!(ring.drops_since_last_drain(), 2);
    let _ = ring.drain_all();
    assert_eq!(ring.drops_since_last_drain(), 0);
}

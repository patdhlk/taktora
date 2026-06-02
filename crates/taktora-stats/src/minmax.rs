//! Exact windowed min/max via a monotonic deque.

/// Private fixed-capacity ring deque of `(sample_index, value)` pairs.
///
/// **Safety contract:** the caller (`MinMaxDeque::record`) must ensure
/// `len < N` before every `push_back`. It guarantees this by draining the
/// deque via `pop_back` (value dominance — the primary room-maker, which
/// also maintains the monotonic invariant) and `pop_front` (window expiry).
/// `push_back` itself performs no bounds check.
struct MonoDeque<const N: usize> {
    buf: [(u64, u64); N],
    head: usize,
    len: usize,
}

impl<const N: usize> MonoDeque<N> {
    const fn new() -> Self {
        Self {
            buf: [(0, 0); N],
            head: 0,
            len: 0,
        }
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    const fn front(&self) -> (u64, u64) {
        self.buf[self.head]
    }

    const fn back(&self) -> (u64, u64) {
        self.buf[(self.head + self.len - 1) % N]
    }

    const fn push_back(&mut self, item: (u64, u64)) {
        self.buf[(self.head + self.len) % N] = item;
        self.len += 1;
    }

    const fn pop_back(&mut self) {
        self.len -= 1;
    }

    const fn pop_front(&mut self) {
        self.head = (self.head + 1) % N;
        self.len -= 1;
    }
}

/// Exact windowed min/max via a monotonic deque. Backs `REQ_0105`.
///
/// Tracks the minimum and maximum over the last `N` recorded samples.
/// Amortised O(1) per `record`; `min`/`max` are O(1). Single-writer
/// (`&mut`), no `unsafe`, no allocation.
pub struct MinMaxDeque<const N: usize> {
    min_d: MonoDeque<N>,
    max_d: MonoDeque<N>,
    next: u64,
}

impl<const N: usize> MinMaxDeque<N> {
    /// Create an empty deque over a window of `N` samples.
    ///
    /// # Panics
    ///
    /// Fails to compile (const-eval assertion) if `N == 0`; a zero-length
    /// window has no meaning and would divide by zero in the ring math.
    #[must_use]
    pub const fn new() -> Self {
        const { assert!(N > 0, "MinMaxDeque window size N must be > 0") }
        Self {
            min_d: MonoDeque::new(),
            max_d: MonoDeque::new(),
            next: 0,
        }
    }

    /// Record one sample. Pops dominated values from the back (maintaining
    /// the monotonic invariant), then evicts any expired front entries
    /// (maintaining the window bound), so the backing arrays never exceed
    /// `N` elements.
    #[allow(clippy::missing_const_for_fn)] // runtime-only mutation; const would be misleading
    pub fn record(&mut self, value: u64) {
        let idx = self.next;
        self.next += 1;
        #[allow(clippy::cast_possible_truncation)] // N ≤ usize::MAX ≤ u64::MAX; window fits u64
        let window_start = (idx + 1).saturating_sub(N as u64);

        // Min deque: monotonically non-decreasing from the front.
        while !self.min_d.is_empty() && self.min_d.back().1 >= value {
            self.min_d.pop_back();
        }
        while !self.min_d.is_empty() && self.min_d.front().0 < window_start {
            self.min_d.pop_front();
        }
        self.min_d.push_back((idx, value));

        // Max deque: monotonically non-increasing from the front.
        while !self.max_d.is_empty() && self.max_d.back().1 <= value {
            self.max_d.pop_back();
        }
        while !self.max_d.is_empty() && self.max_d.front().0 < window_start {
            self.max_d.pop_front();
        }
        self.max_d.push_back((idx, value));
    }

    /// Current windowed minimum, or `None` if no samples recorded.
    #[must_use]
    pub fn min(&self) -> Option<u64> {
        (!self.min_d.is_empty()).then(|| self.min_d.front().1)
    }

    /// Current windowed maximum, or `None` if no samples recorded.
    #[must_use]
    pub fn max(&self) -> Option<u64> {
        (!self.max_d.is_empty()).then(|| self.max_d.front().1)
    }
}

impl<const N: usize> Default for MinMaxDeque<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_reports_none() {
        let d = MinMaxDeque::<8>::new();
        assert_eq!(d.min(), None);
        assert_eq!(d.max(), None);
    }

    #[test]
    fn tracks_min_and_max_within_window() {
        let mut d = MinMaxDeque::<4>::new();
        for v in [5u64, 3, 8, 1] {
            d.record(v);
        }
        // Window = last 4: {5,3,8,1}
        assert_eq!(d.min(), Some(1));
        assert_eq!(d.max(), Some(8));
    }

    #[test]
    fn extrema_age_out_with_the_window() {
        let mut d = MinMaxDeque::<3>::new();
        // Window size 3.
        d.record(100); // [100]
        d.record(2); // [100, 2]
        d.record(50); // [100, 2, 50]  -> min 2, max 100
        assert_eq!(d.min(), Some(2));
        assert_eq!(d.max(), Some(100));
        d.record(40); // window now [2, 50, 40] (100 aged out) -> max 50
        assert_eq!(d.max(), Some(50));
        d.record(60); // window [50, 40, 60] (2 aged out) -> min 40
        assert_eq!(d.min(), Some(40));
        assert_eq!(d.max(), Some(60));
    }

    #[test]
    fn handles_monotonic_increasing_and_decreasing() {
        let mut up = MinMaxDeque::<4>::new();
        for v in 1..=10u64 {
            up.record(v);
        }
        // Last 4: {7,8,9,10}
        assert_eq!(up.min(), Some(7));
        assert_eq!(up.max(), Some(10));

        let mut down = MinMaxDeque::<4>::new();
        for v in (1..=10u64).rev() {
            down.record(v);
        }
        // Last 4: {4,3,2,1}
        assert_eq!(down.min(), Some(1));
        assert_eq!(down.max(), Some(4));
    }
}

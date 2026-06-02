//! Static coupling graph (`REQ_0862`). Topology declared up front; downstream
//! dependent sets precomputed. Fault propagation walks engaged edges only.
//!
//! Expected to be acyclic (a `DAG`), but cycles are tolerated: both the
//! `precompute` fixpoint and the `engaged_downstream` walk terminate on any
//! graph (`visited`/closure sets grow monotonically).

/// Fixed-capacity coupling topology over `N` axes (master -> slave edges).
#[derive(Clone, Debug)]
pub struct CouplingTopology<const N: usize> {
    /// `edges[m]` bit `s` set => declared edge master m -> slave s.
    declared: [u64; N],
    /// `engaged[m]` bit `s` set => that edge is currently active.
    engaged: [u64; N],
    /// Precomputed transitive downstream bitset per axis.
    downstream_bits: [u64; N],
}

impl<const N: usize> CouplingTopology<N> {
    /// Empty topology.
    ///
    /// # Panics
    ///
    /// Panics if `N > 64` (v1 supports at most 64 axes per group).
    #[must_use]
    pub fn new() -> Self {
        assert!(N <= 64, "v1 supports <=64 axes per group");
        Self {
            declared: [0; N],
            engaged: [0; N],
            downstream_bits: [0; N],
        }
    }

    /// Declare a static master -> slave edge.
    ///
    /// `slave` must be `< N`; an out-of-range slave would otherwise set an
    /// unreachable bit that only panics later in `precompute`.
    pub const fn add_edge(&mut self, master: u16, slave: u16) {
        debug_assert!((slave as usize) < N, "slave index out of range for N");
        self.declared[master as usize] |= 1u64 << slave;
    }

    /// Mark a declared edge engaged/disengaged (e.g. `GearIn`/Stop).
    ///
    /// `slave` must be `< N` (see [`Self::add_edge`]).
    pub const fn set_engaged(&mut self, master: u16, slave: u16, on: bool) {
        debug_assert!((slave as usize) < N, "slave index out of range for N");
        let bit = 1u64 << slave;
        if on {
            self.engaged[master as usize] |= bit;
        } else {
            self.engaged[master as usize] &= !bit;
        }
    }

    /// Whether a *direct* declared edge `master -> slave` exists.
    #[must_use]
    pub const fn declared_edge(&self, master: u16, slave: u16) -> bool {
        (self.declared[master as usize] & (1u64 << slave)) != 0
    }

    /// Precompute transitive downstream sets over declared edges. Call
    /// once after all `add_edge`s (`REQ_0862`: topology is static).
    pub fn precompute(&mut self) {
        // Fixpoint over the declared adjacency (N <= 64 so this is cheap).
        for i in 0..N {
            self.downstream_bits[i] = self.declared[i];
        }
        let mut changed = true;
        while changed {
            changed = false;
            for m in 0..N {
                let mut acc = self.downstream_bits[m];
                let mut bits = acc;
                while bits != 0 {
                    // bit indices are < N ≤ 64, so they fit in usize
                    #[allow(clippy::cast_possible_truncation)]
                    let s = bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    acc |= self.downstream_bits[s];
                }
                if acc != self.downstream_bits[m] {
                    self.downstream_bits[m] = acc;
                    changed = true;
                }
            }
        }
    }

    /// Transitive downstream axes over *declared* edges.
    #[must_use]
    pub fn downstream(&self, axis: u16) -> Vec<u16> {
        bits_to_vec(self.downstream_bits[axis as usize])
    }

    /// Transitive downstream axes reachable over *currently-engaged*
    /// edges only — the quickstop set on a fault of `axis` (`REQ_0862`).
    ///
    /// `axis` itself is not included unless an engaged cycle reaches back to
    /// it; the caller handles the faulting axis directly.
    #[must_use]
    pub fn engaged_downstream(&self, axis: u16) -> Vec<u16> {
        let mut visited = 0u64;
        let mut frontier = self.engaged[axis as usize];
        while frontier != 0 {
            visited |= frontier;
            let mut next = 0u64;
            let mut bits = frontier;
            while bits != 0 {
                // bit indices are < N ≤ 64, so they fit in usize
                #[allow(clippy::cast_possible_truncation)]
                let s = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                next |= self.engaged[s];
            }
            frontier = next & !visited;
        }
        bits_to_vec(visited)
    }
}

impl<const N: usize> Default for CouplingTopology<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect the set bits of `b` as axis indices, ascending (LSB-first).
fn bits_to_vec(mut b: u64) -> Vec<u16> {
    #[allow(
        clippy::cast_possible_truncation,
        // count_ones() <= 64, fits in usize.
    )]
    let mut v = Vec::with_capacity(b.count_ones() as usize);
    while b != 0 {
        // bit indices are < N ≤ 64, so they fit in u16
        #[allow(clippy::cast_possible_truncation)]
        v.push(b.trailing_zeros() as u16);
        b &= b - 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    // Graph: 0 -> {1,2}; 1 -> {3}; 4 isolated.
    fn graph() -> CouplingTopology<8> {
        let mut t = CouplingTopology::new();
        t.add_edge(0, 1);
        t.add_edge(0, 2);
        t.add_edge(1, 3);
        t.precompute();
        t
    }

    #[test]
    fn downstream_is_transitive() {
        let t = graph();
        let mut ds = t.downstream(0);
        ds.sort_unstable();
        assert_eq!(ds, vec![1, 2, 3]);
        assert_eq!(t.downstream(4), Vec::<u16>::new());
    }

    #[test]
    fn fault_set_intersects_engaged_edges() {
        let mut t = graph();
        // only edge 0->1 engaged; 0->2 and 1->3 disengaged.
        t.set_engaged(0, 1, true);
        let mut s = t.engaged_downstream(0);
        s.sort_unstable();
        assert_eq!(s, vec![1]); // 2 and 3 not reached (their edges disengaged)
    }
}

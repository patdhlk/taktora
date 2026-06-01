//! Electronic camming: a slave whose position is a piecewise-polynomial
//! function of the **master position**.
//!
//! Where a [`Gear`](crate::couple::Gear) ties the slave to the master through a
//! single scalar ratio, a *cam* ties it through an arbitrary curve
//! `slave_pos = P(s)`, where `s` is the master position mapped into the cam's
//! domain. The classic mechanical analogue is a shaped disc (the cam) whose
//! profile a follower rides; here the disc is a table of polynomial pieces.
//!
//! # Master is the independent variable
//!
//! The cam is *purely master-driven*: it owns no clock and `dt` is ignored.
//! Position comes straight from the profile, and the time derivatives follow
//! from the chain rule (the master is what moves in time):
//!
//! ```text
//!   slave_pos = P(s)
//!   slave_vel = P'(s) · master_vel
//!   slave_acc = P''(s) · master_vel²  +  P'(s) · master_acc
//! ```
//!
//! The `master_vel²` term is the cam-specific contribution: even when the
//! master coasts at constant velocity (`master_acc = 0`) the slave still
//! accelerates wherever the profile curves (`P'' ≠ 0`).
//!
//! # Why quintic pieces?
//!
//! Each [`CamSegment`] stores its slave-position polynomial as a **degree-5
//! (quintic)** in the local master coordinate `u = s − master_start`. Six
//! coefficients are exactly enough to match position, slope (`P'`) **and**
//! curvature (`P''`) at both ends of a piece, so a table assembled with matched
//! seams is **C²** in the master coordinate — no step in `P'` or `P''`, hence
//! no velocity or acceleration jump as the master crosses a seam (for a master
//! moving smoothly).
//!
//! # Coefficient layout
//!
//! [`CamSegment::coeffs`] is `[c0, c1, c2, c3, c4, c5]`, the Horner-ordered
//! coefficients of
//!
//! ```text
//!   P(u)  = c0 + c1 u + c2 u² + c3 u³ + c4 u⁴ + c5 u⁵
//!   P'(u) =      c1 + 2 c2 u + 3 c3 u² + 4 c4 u³ + 5 c5 u⁴
//!   P''(u)=           2 c2   + 6 c3 u + 12 c4 u² + 20 c5 u³
//! ```
//!
//! with `u = s − master_start` the offset into the piece. A straight gearing
//! `slave = k · master` is the single-segment table `[0, k, 0, 0, 0, 0]`
//! starting at `master_start = 0` — see [`CamSegment::linear`].
//!
//! # Deferred
//!
//! Runtime / dynamic cam loading and `heapless`-backed tables are explicitly
//! out of scope: a [`CamTable`] wraps a `&'static [CamSegment]`, so tables are
//! built at compile time and the whole generator stays `Copy` and
//! allocation-free.

use crate::math::{is_positive, rem_euclid};
use crate::state::AxisState;

/// One quintic piece of a cam profile.
///
/// It is valid over a master-position interval that begins at
/// [`master_start`](Self::master_start) and runs until the next segment's start
/// (or the table [`period`](CamTable::period) for the last).
///
/// The polynomial is expressed in the **local** master coordinate
/// `u = master_pos − master_start`, so a piece's coefficients are independent of
/// where it sits in the table. See the [module docs](self) for the coefficient
/// layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamSegment {
    /// Horner-ordered quintic coefficients `[c0, c1, c2, c3, c4, c5]` in the
    /// local master coordinate `u = master_pos − master_start`.
    coeffs: [f64; 6],
    /// Master position at which this segment begins. Segments in a
    /// [`CamTable`] are sorted ascending by this value.
    master_start: f64,
}

impl CamSegment {
    /// A quintic segment from explicit coefficients and its start position.
    ///
    /// `coeffs` is `[c0, c1, c2, c3, c4, c5]` of the slave-position polynomial
    /// in the local coordinate `u = master_pos − master_start` (see the
    /// [module docs](self)). `const` so whole tables can be `static`.
    #[inline]
    #[must_use]
    pub const fn new(coeffs: [f64; 6], master_start: f64) -> Self {
        Self {
            coeffs,
            master_start,
        }
    }

    /// A straight (degree-1) segment `slave = slope · u + intercept` starting at
    /// `master_start`. With `master_start = 0` and `intercept = 0` this is the
    /// gearing-equivalent single-segment cam `slave = slope · master`.
    #[inline]
    #[must_use]
    pub const fn linear(slope: f64, intercept: f64, master_start: f64) -> Self {
        Self::new([intercept, slope, 0.0, 0.0, 0.0, 0.0], master_start)
    }

    /// Master position at which this segment begins.
    #[inline]
    #[must_use]
    pub const fn master_start(&self) -> f64 {
        self.master_start
    }

    /// The quintic coefficients `[c0, c1, c2, c3, c4, c5]`.
    #[inline]
    #[must_use]
    pub const fn coeffs(&self) -> [f64; 6] {
        self.coeffs
    }

    /// Evaluate `P(u)`, `P'(u)`, `P''(u)` at local offset `u` by Horner.
    ///
    /// Returns `(value, first_derivative, second_derivative)` of the
    /// slave-position polynomial with respect to the master coordinate.
    fn eval(&self, u: f64) -> (f64, f64, f64) {
        let [c0, c1, c2, c3, c4, c5] = self.coeffs;
        // P(u) by Horner.
        let p = c0 + u * (c1 + u * (c2 + u * (c3 + u * (c4 + u * c5))));
        // P'(u) by Horner: c1 + 2c2 u + 3c3 u² + 4c4 u³ + 5c5 u⁴.
        let dp = c1 + u * (2.0 * c2 + u * (3.0 * c3 + u * (4.0 * c4 + u * (5.0 * c5))));
        // P''(u) by Horner: 2c2 + 6c3 u + 12c4 u² + 20c5 u³.
        let ddp = 2.0 * c2 + u * (6.0 * c3 + u * (12.0 * c4 + u * (20.0 * c5)));
        (p, dp, ddp)
    }
}

/// A compile-time cam profile: a sorted slice of quintic [`CamSegment`]s that
/// repeats every [`period`](Self::period) of master travel.
///
/// The segments must be **sorted ascending** by
/// [`master_start`](CamSegment::master_start), with the first starting at
/// (or before) `0.0` and all starts inside `[0, period)`.
///
/// The table tiles the master axis by wrapping the master position into
/// `[0, period)`. Wrapping a `&'static [_]` keeps the table `Copy`, so it can
/// live in a `Cam` inside the `Copy` [`Motion`](crate::Motion) enum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamTable {
    segments: &'static [CamSegment],
    period: f64,
}

impl CamTable {
    /// Wrap a sorted segment slice with the given master `period`.
    ///
    /// The caller guarantees `segments` is sorted ascending by `master_start`
    /// and that `period > 0`. A non-positive `period` or an empty slice yields a
    /// degenerate table that [`Cam`] handles by holding (never panicking), so
    /// this stays `const` and total.
    #[inline]
    #[must_use]
    pub const fn new(segments: &'static [CamSegment], period: f64) -> Self {
        Self { segments, period }
    }

    /// The master travel over which the profile repeats.
    #[inline]
    #[must_use]
    pub const fn period(&self) -> f64 {
        self.period
    }

    /// The segments backing this table.
    #[inline]
    #[must_use]
    pub const fn segments(&self) -> &'static [CamSegment] {
        self.segments
    }

    /// `true` if the table cannot be evaluated (no segments or a non-positive
    /// period) and a [`Cam`] should hold instead.
    #[inline]
    fn is_degenerate(&self) -> bool {
        self.segments.is_empty() || !is_positive(self.period)
    }

    /// Index of the segment whose interval contains the wrapped master position
    /// `s ∈ [0, period)`, via a bounded binary search for the last segment with
    /// `master_start <= s`.
    ///
    /// Assumes a non-empty, ascending-sorted table (callers guard the
    /// degenerate case first). Bounded by `⌈log2(N)⌉` iterations and
    /// panic-free: it indexes only `lo`/`mid` that stay within `[0, len)`.
    fn segment_index(&self, s: f64) -> usize {
        let segs = self.segments;
        // Invariant: the answer is in `[lo, hi)`. `segs[lo]` is known to start
        // at or before `s` once we've established `s >= segs[0].master_start`;
        // before that the wrap guarantees the first segment owns the prefix.
        let mut lo = 0usize;
        let mut hi = segs.len();
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if segs[mid].master_start <= s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

/// Electronic-cam generator: drives a slave along a [`CamTable`] as a function
/// of the master position.
///
/// Construct with [`Cam::new`]; drive with [`Cam::update`] once per control
/// cycle. The slave's velocity and acceleration are produced by the chain rule
/// (see the [module docs](self)), so the coupling is same-cycle coherent — the
/// master's set-state for *this* cycle yields the slave's set-state for the same
/// cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cam {
    table: CamTable,
}

impl Cam {
    /// A cam following the given profile table.
    #[inline]
    #[must_use]
    pub const fn new(table: CamTable) -> Self {
        Self { table }
    }

    /// The profile table this cam follows.
    #[inline]
    #[must_use]
    pub const fn table(&self) -> CamTable {
        self.table
    }

    /// The master travel over which the profile repeats.
    #[inline]
    #[must_use]
    pub const fn period(&self) -> f64 {
        self.table.period
    }

    /// Compute the slave set-state from this cycle's `master` state.
    ///
    /// `dt` is ignored: a cam carries no clock, it is purely master-driven.
    ///
    /// * **No master this cycle** (`master` is `None`): the cam **holds at the
    ///   profile origin** — it evaluates at master position `0.0` with zero
    ///   master velocity/acceleration, i.e. `AxisState::at(P(0))`. (A degenerate
    ///   or empty table holds at `AxisState::ZERO`.)
    /// * **Master present**: the master position is wrapped into `[0, period)`
    ///   via [`rem_euclid`](crate::math::rem_euclid), the owning segment is
    ///   found by a bounded binary search, the quintic is evaluated by Horner,
    ///   and the chain rule produces velocity and acceleration.
    ///
    /// Allocation-free, panic-free, and bounded: the binary search runs in
    /// `O(log N)` and a degenerate table yields a safe hold.
    #[must_use]
    pub fn update(&self, _dt: f64, master: Option<AxisState>) -> AxisState {
        if self.table.is_degenerate() {
            // No usable profile: hold at the origin rather than indexing an
            // empty slice or dividing by a zero period.
            return AxisState::ZERO;
        }

        let Some(m) = master else {
            // Uncoupled cam: hold at the profile value for master position 0.
            let s0 = rem_euclid(0.0, self.table.period);
            let seg = &self.table.segments[self.table.segment_index(s0)];
            let (p, _, _) = seg.eval(s0 - seg.master_start);
            return AxisState::at(p);
        };

        // Map the master position into one profile period.
        let s = rem_euclid(m.pos, self.table.period);
        let seg = &self.table.segments[self.table.segment_index(s)];
        let u = s - seg.master_start;
        let (p, dp, ddp) = seg.eval(u);

        // Chain rule: master is the independent variable.
        //   vel = P'·ṁ
        //   acc = P''·ṁ² + P'·m̈
        let vel = dp * m.vel;
        let acc = ddp * m.vel * m.vel + dp * m.acc;
        AxisState::new(p, vel, acc)
    }
}

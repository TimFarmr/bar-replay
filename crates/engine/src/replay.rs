//! The replay cursor.
//!
//! **The cursor is a timestamp, never an array index** (spec §5.1). Navigation
//! binary-searches the list of base-bar open times, so nothing here can drift
//! out of step with the data the way a stored index would after a refetch.
//!
//! The cursor always sits on the open time of a base bar that exists. Stepping
//! therefore skips gaps rather than walking through empty minutes, and a jump
//! into the middle of a weekend lands on the last bar before it.

use replay_core::Timestamp;

pub struct Replay {
    /// Open times of every base bar in the session range, ascending and unique.
    times: Vec<Timestamp>,
    cursor: Timestamp,
}

impl Replay {
    /// `times` must be ascending and non-empty; the cursor starts at the first
    /// bar, which is the honest starting point: nothing has been revealed yet
    /// beyond the opening bar.
    pub fn new(times: Vec<Timestamp>) -> Option<Replay> {
        debug_assert!(times.windows(2).all(|w| w[0] < w[1]), "times must ascend");
        let cursor = *times.first()?;
        Some(Replay { times, cursor })
    }

    pub fn cursor(&self) -> Timestamp {
        self.cursor
    }

    pub fn first(&self) -> Timestamp {
        self.times[0]
    }

    pub fn last(&self) -> Timestamp {
        self.times[self.times.len() - 1]
    }

    pub fn bars(&self) -> usize {
        self.times.len()
    }

    /// How many bars have been revealed, 1-based. For progress display only —
    /// never an input to navigation.
    pub fn position(&self) -> usize {
        self.index_of_cursor() + 1
    }

    pub fn at_end(&self) -> bool {
        self.cursor >= self.last()
    }

    pub fn at_start(&self) -> bool {
        self.cursor <= self.first()
    }

    /// Index of the newest bar at or before the cursor. Computed on demand,
    /// never stored.
    fn index_of_cursor(&self) -> usize {
        // partition_point gives the count of bars with ts <= cursor; the cursor
        // always sits on a real bar, so this is at least 1.
        self.times.partition_point(|t| *t <= self.cursor) - 1
    }

    /// Reveal the next bar. Returns false if the cursor is already at the end.
    pub fn step_forward(&mut self) -> bool {
        self.advance(1) > 0
    }

    /// Hide the newest bar. Returns false if the cursor is already at the start.
    pub fn step_back(&mut self) -> bool {
        self.rewind(1) > 0
    }

    /// Move forward up to `n` bars, returning how many were actually revealed.
    pub fn advance(&mut self, n: usize) -> usize {
        let from = self.index_of_cursor();
        let to = (from + n).min(self.times.len() - 1);
        self.cursor = self.times[to];
        to - from
    }

    /// Move back up to `n` bars, returning how many were actually hidden.
    pub fn rewind(&mut self, n: usize) -> usize {
        let from = self.index_of_cursor();
        let to = from.saturating_sub(n);
        self.cursor = self.times[to];
        from - to
    }

    /// Put the cursor on the newest bar at or before `target`.
    ///
    /// A target inside a gap lands on the last bar before the gap rather than
    /// inventing a bar; a target before the range clamps to the first bar.
    pub fn jump_to(&mut self, target: Timestamp) -> Timestamp {
        let count = self.times.partition_point(|t| *t <= target);
        self.cursor = if count == 0 {
            self.times[0]
        } else {
            self.times[count - 1]
        };
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(list: &[i64]) -> Vec<Timestamp> {
        list.iter()
            .map(|m| Timestamp(m * Timestamp::MINUTE))
            .collect()
    }

    /// A gap between minute 2 and minute 10, as a weekend would look.
    fn gapped() -> Replay {
        Replay::new(minutes(&[0, 1, 2, 10, 11, 12])).unwrap()
    }

    #[test]
    fn a_new_replay_starts_on_the_first_bar_revealing_nothing_else() {
        let r = gapped();
        assert_eq!(r.cursor(), Timestamp(0));
        assert!(r.at_start());
        assert!(!r.at_end());
        assert_eq!(r.position(), 1);
        assert_eq!(r.bars(), 6);
    }

    #[test]
    fn stepping_forward_skips_a_gap_instead_of_walking_empty_minutes() {
        let mut r = gapped();
        r.advance(2);
        assert_eq!(r.cursor(), Timestamp(2 * Timestamp::MINUTE));
        assert!(r.step_forward());
        assert_eq!(
            r.cursor(),
            Timestamp(10 * Timestamp::MINUTE),
            "the eight missing minutes are a gap, not bars to step through"
        );
    }

    #[test]
    fn stepping_back_is_the_exact_inverse_of_stepping_forward() {
        let mut r = gapped();
        let mut seen = vec![r.cursor()];
        while r.step_forward() {
            seen.push(r.cursor());
        }
        assert_eq!(seen.len(), 6);
        for want in seen.iter().rev().skip(1) {
            assert!(r.step_back());
            assert_eq!(r.cursor(), *want);
        }
        assert!(r.at_start());
        assert!(!r.step_back(), "cannot step behind the first bar");
    }

    #[test]
    fn the_cursor_stops_at_both_ends_rather_than_wrapping_or_overrunning() {
        let mut r = gapped();
        assert_eq!(r.advance(999), 5);
        assert!(r.at_end());
        assert!(!r.step_forward());
        assert_eq!(r.cursor(), Timestamp(12 * Timestamp::MINUTE));
        assert_eq!(r.rewind(999), 5);
        assert!(r.at_start());
    }

    #[test]
    fn jumping_into_a_gap_lands_on_the_last_real_bar_before_it() {
        let mut r = gapped();
        let landed = r.jump_to(Timestamp(7 * Timestamp::MINUTE));
        assert_eq!(landed, Timestamp(2 * Timestamp::MINUTE));
        assert_eq!(r.position(), 3);
    }

    #[test]
    fn jumping_outside_the_range_clamps_to_the_ends() {
        let mut r = gapped();
        assert_eq!(r.jump_to(Timestamp(-1)), Timestamp(0));
        assert_eq!(
            r.jump_to(Timestamp(999 * Timestamp::MINUTE)),
            Timestamp(12 * Timestamp::MINUTE)
        );
    }

    #[test]
    fn jumping_to_an_exact_bar_time_selects_that_bar() {
        let mut r = gapped();
        assert_eq!(
            r.jump_to(Timestamp(10 * Timestamp::MINUTE)),
            Timestamp(10 * Timestamp::MINUTE)
        );
        assert_eq!(r.position(), 4);
    }

    #[test]
    fn a_single_bar_session_is_both_start_and_end() {
        let mut r = Replay::new(minutes(&[5])).unwrap();
        assert!(r.at_start() && r.at_end());
        assert!(!r.step_forward() && !r.step_back());
    }

    #[test]
    fn an_empty_range_has_no_replay_at_all() {
        assert!(Replay::new(Vec::new()).is_none());
    }
}

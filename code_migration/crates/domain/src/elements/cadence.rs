use chrono::{DateTime, Timelike, Utc};

/// A minute-of-hour slot, in `0..=59`.
///
/// The scheduler ticks once a minute; on each tick it constructs the current
/// `PublishBlock` and checks which Posters fire via [`PublishBlock::fires_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishBlock(u8);

#[derive(Debug, thiserror::Error)]
pub enum PublishBlockError {
    #[error("PublishBlock out of range (expected 0..=59): {0}")]
    OutOfRange(u8),
}

impl PublishBlock {
    pub fn new(value: u8) -> Result<Self, PublishBlockError> {
        if value > 59 {
            return Err(PublishBlockError::OutOfRange(value));
        }
        Ok(Self(value))
    }
}

impl TryFrom<u8> for PublishBlock {
    type Error = PublishBlockError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<&DateTime<Utc>> for PublishBlock {
    fn from(value: &DateTime<Utc>) -> Self {
        Self(value.minute() as u8)
    }
}

impl AsRef<u8> for PublishBlock {
    fn as_ref(&self) -> &u8 {
        &self.0
    }
}

/// A Poster's posting cadence, in whole minutes, restricted to divisors of 60.
///
/// Restricting to divisors of 60 keeps every interval aligned to the top of the
/// hour: a 5-minute Poster fires at HH:00, HH:05, HH:10, … with no irregular
/// boundary at HH:00. Valid values: `{1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30, 60}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostInterval(u8);

#[derive(Debug, thiserror::Error)]
pub enum PostIntervalError {
    #[error("PostInterval must be a divisor of 60: got {0}")]
    NotDivisorOf60(u8),
}

const VALID_INTERVALS: &[u8] = &[1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30, 60];

impl PostInterval {
    pub fn new(value: u8) -> Result<Self, PostIntervalError> {
        if !VALID_INTERVALS.contains(&value) {
            return Err(PostIntervalError::NotDivisorOf60(value));
        }
        Ok(Self(value))
    }
}

impl TryFrom<u8> for PostInterval {
    type Error = PostIntervalError;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl AsRef<u8> for PostInterval {
    fn as_ref(&self) -> &u8 {
        &self.0
    }
}

impl PublishBlock {
    /// True when this tick is a firing tick for a Poster on the given interval.
    pub fn fires_for(&self, interval: &PostInterval) -> bool {
        self.0.is_multiple_of(interval.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn publish_block_accepts_full_range() {
        for n in 0..=59u8 {
            PublishBlock::try_from(n).unwrap();
        }
    }

    #[test]
    fn publish_block_rejects_out_of_range() {
        assert!(matches!(
            PublishBlock::try_from(60),
            Err(PublishBlockError::OutOfRange(60))
        ));
        assert!(matches!(
            PublishBlock::try_from(255),
            Err(PublishBlockError::OutOfRange(255))
        ));
    }

    #[test]
    fn publish_block_from_datetime() {
        let dt = Utc.with_ymd_and_hms(2026, 5, 17, 14, 23, 45).unwrap();
        let block = PublishBlock::from(&dt);
        assert_eq!(*block.as_ref(), 23);
    }

    #[test]
    fn post_interval_accepts_divisors_of_60() {
        for n in VALID_INTERVALS {
            PostInterval::try_from(*n).unwrap();
        }
    }

    #[test]
    fn post_interval_rejects_non_divisors() {
        for n in [0u8, 7, 8, 9, 11, 13, 25, 45, 59, 61, 120] {
            assert!(matches!(
                PostInterval::try_from(n),
                Err(PostIntervalError::NotDivisorOf60(_))
            ));
        }
    }

    #[test]
    fn fires_for_block_zero_fires_for_all_intervals() {
        let block = PublishBlock::new(0).unwrap();
        for n in VALID_INTERVALS {
            let interval = PostInterval::new(*n).unwrap();
            assert!(block.fires_for(&interval), "block 0 should fire for {n}");
        }
    }

    #[test]
    fn fires_for_block_fifteen() {
        let block = PublishBlock::new(15).unwrap();
        // 15 is divisible by 1, 3, 5, 15; not by 2, 4, 6, 10, 12, 20, 30, 60.
        let fires: &[u8] = &[1, 3, 5, 15];
        let doesnt: &[u8] = &[2, 4, 6, 10, 12, 20, 30, 60];
        for n in fires {
            let i = PostInterval::new(*n).unwrap();
            assert!(block.fires_for(&i), "block 15 should fire for {n}");
        }
        for n in doesnt {
            let i = PostInterval::new(*n).unwrap();
            assert!(!block.fires_for(&i), "block 15 should NOT fire for {n}");
        }
    }

    #[test]
    fn fires_for_block_seven_only_fires_for_one() {
        let block = PublishBlock::new(7).unwrap();
        for n in VALID_INTERVALS {
            let interval = PostInterval::new(*n).unwrap();
            let expected = *n == 1;
            assert_eq!(
                block.fires_for(&interval),
                expected,
                "block 7 with interval {n}: expected {expected}"
            );
        }
    }
}

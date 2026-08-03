//! Bounded ring buffers (SPEC §13, allocation-bounded before allocation).
//!
//! Two overflow policies coexist and MUST NEVER be merged (trap BXS-W-03 /
//! LAW-17 善意の一本化は破壊である):
//! - `evicting_push` — derived data (market series): oldest entry may fall out.
//! - `try_push`      — records (journal tail, decision log): full = typed
//!   rejection; the caller must drain explicitly. Records are never silently
//!   dropped (第八律).

use std::collections::VecDeque;

/// Hard cap on any ring capacity: keeps a caller bug from turning a "bounded"
/// buffer into an unbounded allocation (§16: limits apply before allocation).
pub const MAX_RING_CAPACITY: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingConfigError {
    ZeroCapacity,
    CapacityAboveMax { requested: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingFull;

#[derive(Debug, PartialEq, Eq)]
pub enum RingPush<T> {
    Stored,
    Evicted(T),
}

#[derive(Debug, Clone)]
pub struct FixedRing<T> {
    buf: VecDeque<T>,
    capacity: usize,
}

impl<T> FixedRing<T> {
    pub fn new(capacity: usize) -> Result<Self, RingConfigError> {
        if capacity == 0 {
            return Err(RingConfigError::ZeroCapacity);
        }
        if capacity > MAX_RING_CAPACITY {
            return Err(RingConfigError::CapacityAboveMax {
                requested: capacity,
            });
        }
        Ok(Self {
            // Single up-front allocation; VecDeque may round up by a bounded
            // constant factor but never grows afterwards (pushes are gated).
            buf: VecDeque::with_capacity(capacity),
            capacity,
        })
    }

    /// Derived-data policy: overwrite oldest when full, returning the evicted
    /// element so the caller can observe (never use for records — BXS-W-03).
    pub fn evicting_push(&mut self, item: T) -> RingPush<T> {
        if self.buf.len() >= self.capacity {
            let evicted = self.buf.pop_front();
            self.buf.push_back(item);
            return match evicted {
                Some(old) => RingPush::Evicted(old),
                // len >= capacity >= 1 implies pop_front yielded a value;
                // treat the impossible arm as a plain store (total function).
                None => RingPush::Stored,
            };
        }
        self.buf.push_back(item);
        RingPush::Stored
    }

    /// Record policy: full = typed rejection, caller must drain explicitly.
    pub fn try_push(&mut self, item: T) -> Result<(), RingFull> {
        if self.buf.len() >= self.capacity {
            return Err(RingFull);
        }
        self.buf.push_back(item);
        Ok(())
    }

    /// Explicit flush: removes and returns everything, oldest first.
    pub fn drain_all(&mut self) -> Vec<T> {
        self.buf.drain(..).collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.buf.len() >= self.capacity
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub fn latest(&self) -> Option<&T> {
        self.buf.back()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buf.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("test setup failed: {e:?}"),
        }
    }

    #[test]
    fn zero_and_oversized_capacity_rejected() {
        assert!(matches!(
            FixedRing::<u8>::new(0),
            Err(RingConfigError::ZeroCapacity)
        ));
        assert!(matches!(
            FixedRing::<u8>::new(MAX_RING_CAPACITY + 1),
            Err(RingConfigError::CapacityAboveMax { requested }) if requested == MAX_RING_CAPACITY + 1
        ));
    }

    #[test]
    fn evicting_push_evicts_oldest_in_order() {
        let mut r = ok(FixedRing::new(3));
        assert_eq!(r.evicting_push(1), RingPush::Stored);
        assert_eq!(r.evicting_push(2), RingPush::Stored);
        assert_eq!(r.evicting_push(3), RingPush::Stored);
        assert_eq!(r.evicting_push(4), RingPush::Evicted(1));
        assert_eq!(r.evicting_push(5), RingPush::Evicted(2));
        let content: Vec<i32> = r.iter().copied().collect();
        assert_eq!(content, vec![3, 4, 5]);
        assert_eq!(r.latest(), Some(&5));
    }

    #[test]
    fn try_push_rejects_when_full_then_drain_reopens() {
        let mut r = ok(FixedRing::new(2));
        ok(r.try_push(10));
        ok(r.try_push(20));
        assert!(r.is_full());
        assert!(matches!(r.try_push(30), Err(RingFull)));
        let drained = r.drain_all();
        assert_eq!(drained, vec![10, 20]);
        assert!(r.is_empty());
        ok(r.try_push(30));
        assert_eq!(r.len(), 1);
    }
}

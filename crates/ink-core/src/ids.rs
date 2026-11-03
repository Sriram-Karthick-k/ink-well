//! Deterministic ID generation.
//!
//! The core has no random-number source of its own, so the same session replays identically on every
//! platform. The app seeds an [`IdGen`] (from the clock, say) and hands it to whatever creates ids.

use uuid::{Builder, Uuid};

/// Generates unique, version-4-shaped UUIDs from a seed (splitmix64). Not cryptographic.
#[derive(Debug, Clone)]
pub struct IdGen {
    state: u64,
}

impl IdGen {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The next id.
    pub fn next_id(&mut self) -> Uuid {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&self.next_u64().to_le_bytes());
        bytes[8..].copy_from_slice(&self.next_u64().to_le_bytes());
        Builder::from_random_bytes(bytes).into_uuid()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let (mut a, mut b) = (IdGen::new(7), IdGen::new(7));
        for _ in 0..10 {
            assert_eq!(a.next_id(), b.next_id());
        }
    }

    #[test]
    fn ids_are_unique_and_version_4() {
        let mut ids_gen = IdGen::new(1);
        let ids: Vec<Uuid> = (0..1000).map(|_| ids_gen.next_id()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 1000);
        assert!(ids.iter().all(|id| id.get_version_num() == 4));
    }
}

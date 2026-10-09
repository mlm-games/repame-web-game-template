//! Deterministic random stream. Every draw the game needs comes from here so
//! a run can be reproduced from its seed in a test.

use std::fmt;

use rand::rngs::SmallRng;
use rand::{RngExt as _, SeedableRng as _};

#[derive(Clone)]
pub struct Rng(SmallRng);

impl fmt::Debug for Rng {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Rng(..)")
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(SmallRng::seed_from_u64(seed))
    }

    /// `[0, 1)`.
    pub fn randf(&mut self) -> f32 {
        self.0.random::<f32>()
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.randf()
    }
}

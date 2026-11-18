use super::VM;
use std::time::{SystemTime, UNIX_EPOCH};

const STEP: u64 = 0x9E3779B97F4A7C15;
const STATE_MASK: u64 = (1 << 48) - 1;
const STATE_RANGE: u64 = 1 << 48;

pub(super) fn initial_random_state() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    u64::try_from(nanos & u128::from(u64::MAX)).expect("masked timestamp fits u64") & STATE_MASK
}

impl VM {
    pub fn set_random_seed(&mut self, seed: u64) {
        self.random_state = seed & STATE_MASK;
        self.random_seed = self.random_state;
    }

    pub fn random_state(&self) -> u64 {
        self.random_state
    }

    pub fn random_seed(&self) -> u64 {
        self.random_seed
    }

    pub fn set_random_state(&mut self, state: u64) {
        self.random_state = state & STATE_MASK;
    }

    pub(crate) fn random_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (STATE_RANGE as f64);
        self.next_random_u48() as f64 * SCALE
    }

    pub(crate) fn random_i64_inclusive(&mut self, min: i64, max: i64) -> i64 {
        let width = u128::try_from(i128::from(max) - i128::from(min) + 1)
            .expect("random range width is positive");
        let offset = match u64::try_from(width) {
            Ok(narrow) if narrow <= STATE_RANGE => u128::from(self.random_below(narrow)),
            _ => self.random_wide_below(width),
        };
        i64::try_from(i128::from(min) + offset as i128).expect("random value stays in its range")
    }

    // two draws give 96 bits, enough to sample any i64 range without bias
    fn random_wide_below(&mut self, bound: u128) -> u128 {
        let range: u128 = 1 << 96;
        let limit = range - range % bound;
        loop {
            let value = (u128::from(self.next_random_u48()) << 48) | u128::from(self.next_random_u48());
            if value < limit {
                return value % bound;
            }
        }
    }

    fn next_random_u48(&mut self) -> u64 {
        self.random_state = self.random_state.wrapping_add(STEP) & STATE_MASK;
        let mut value = self.random_state;
        value ^= value >> 21;
        value ^= (value << 17) & STATE_MASK;
        (value ^ (value >> 9)) & STATE_MASK
    }

    fn random_below(&mut self, bound: u64) -> u64 {
        debug_assert!(bound > 0 && bound <= STATE_RANGE);
        let limit = STATE_RANGE - STATE_RANGE % bound;
        loop {
            let value = self.next_random_u48();
            if value < limit {
                return value % bound;
            }
        }
    }
}

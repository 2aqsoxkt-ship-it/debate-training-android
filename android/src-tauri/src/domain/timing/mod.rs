pub const DEFAULT_SPEECH_RATE: u32 = 240;

pub fn word_cap(duration_seconds: u32, speech_rate_per_minute: u32) -> u32 {
    let product = u64::from(duration_seconds).saturating_mul(u64::from(speech_rate_per_minute));
    u32::try_from(product / 60).unwrap_or(u32::MAX).max(1)
}

#[derive(Debug, Clone)]
pub struct FakeClock {
    now_ms: u64,
}

impl FakeClock {
    pub fn new() -> Self {
        Self { now_ms: 0 }
    }

    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    pub fn advance(&mut self, milliseconds: u64) {
        self.now_ms = self.now_ms.saturating_add(milliseconds);
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{word_cap, FakeClock, DEFAULT_SPEECH_RATE};

    #[test]
    fn converts_duration_at_the_default_rate() {
        assert_eq!(word_cap(60, DEFAULT_SPEECH_RATE), 240);
        assert_eq!(word_cap(30, DEFAULT_SPEECH_RATE), 120);
        assert_eq!(word_cap(1, DEFAULT_SPEECH_RATE), 4);
    }

    #[test]
    fn ten_minutes_of_ticks_do_not_drift() {
        let mut clock = FakeClock::new();
        let mut elapsed = 0_u64;
        for _ in 0..600 {
            clock.advance(1_000);
            elapsed += 1_000;
        }
        assert_eq!(clock.now_ms(), 600_000);
        assert_eq!(elapsed, clock.now_ms());
    }

    #[test]
    fn fake_clock_only_moves_when_advanced() {
        let mut clock = FakeClock::new();
        assert_eq!(clock.now_ms(), 0);
        clock.advance(1_500);
        assert_eq!(clock.now_ms(), 1_500);
    }
}

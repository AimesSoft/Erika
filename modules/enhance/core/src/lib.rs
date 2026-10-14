//! Engine of the enhance module: plain Rust, tested without a window.

use bern::CommandError;
use erika_enhance_protocol::enhance::{self, State};

pub struct EnhanceEngine {
    state: State,
}

impl Default for EnhanceEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl enhance::Commands for EnhanceEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl enhance::Engine for EnhanceEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use enhance::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = EnhanceEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

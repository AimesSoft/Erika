//! Engine of the sync module: plain Rust, tested without a window.

use bern::CommandError;
use erika_sync_protocol::sync::{self, State};

pub struct SyncEngine {
    state: State,
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl sync::Commands for SyncEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl sync::Engine for SyncEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = SyncEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

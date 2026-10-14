//! Engine of the sources module: plain Rust, tested without a window.

use bern::CommandError;
use erika_sources_protocol::sources::{self, State};

pub struct SourcesEngine {
    state: State,
}

impl Default for SourcesEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl sources::Commands for SourcesEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl sources::Engine for SourcesEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sources::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = SourcesEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

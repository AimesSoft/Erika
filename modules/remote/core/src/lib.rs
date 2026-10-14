//! Engine of the remote module: plain Rust, tested without a window.

use bern::CommandError;
use erika_remote_protocol::remote::{self, State};

pub struct RemoteEngine {
    state: State,
}

impl Default for RemoteEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl remote::Commands for RemoteEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl remote::Engine for RemoteEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = RemoteEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

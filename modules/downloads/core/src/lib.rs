//! Engine of the downloads module: plain Rust, tested without a window.

use bern::CommandError;
use erika_downloads_protocol::downloads::{self, State};

pub struct DownloadsEngine {
    state: State,
}

impl Default for DownloadsEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl downloads::Commands for DownloadsEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl downloads::Engine for DownloadsEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use downloads::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = DownloadsEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

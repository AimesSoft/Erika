//! Engine of the library module: plain Rust, tested without a window.

use bern::CommandError;
use erika_library_protocol::library::{self, State};

pub struct LibraryEngine {
    state: State,
}

impl Default for LibraryEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl library::Commands for LibraryEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl library::Engine for LibraryEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = LibraryEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

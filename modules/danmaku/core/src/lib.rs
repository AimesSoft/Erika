//! Engine of the danmaku module: plain Rust, tested without a window.

use bern::CommandError;
use erika_danmaku_protocol::danmaku::{self, State};

pub struct DanmakuEngine {
    state: State,
}

impl Default for DanmakuEngine {
    fn default() -> Self {
        Self {
            state: State { count: 0 },
        }
    }
}

impl danmaku::Commands for DanmakuEngine {
    fn add_one(&mut self) -> Result<(), CommandError> {
        self.state.count += 1;
        Ok(())
    }
}

impl danmaku::Engine for DanmakuEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use danmaku::Commands;

    #[test]
    fn the_button_counts() {
        let mut engine = DanmakuEngine::default();
        engine.add_one().unwrap();
        assert_eq!(engine.state.count, 1);
    }
}

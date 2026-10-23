//! Registers account availability with Bern's optional-module lifecycle.

use erika_account_protocol::account::{self, State};

pub struct AccountEngine {
    state: State,
}

impl Default for AccountEngine {
    fn default() -> Self {
        Self { state: State {} }
    }
}

impl account::Commands for AccountEngine {}

impl account::Engine for AccountEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

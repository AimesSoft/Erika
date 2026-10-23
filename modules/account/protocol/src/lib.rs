//! Account availability is a module; shared account state belongs to the shell.

#[bern::protocol(namespace = "account", version = 1)]
pub mod account {
    #[state]
    pub struct State {}

    #[commands]
    pub trait Commands {}
}

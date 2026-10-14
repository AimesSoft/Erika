//! Protocol of the sources module: the state it shows and the commands that
//! change it. Other modules reach it only through what is declared here.

#[bern::protocol(namespace = "sources", version = 1)]
pub mod sources {
    use bern::prelude::*;

    #[state]
    pub struct State {
        /// How many times the button was pressed.
        pub count: i64,
    }

    #[commands]
    pub trait Commands {
        /// Add one to the count.
        fn add_one(&mut self) -> Result<(), CommandError>;
    }
}

//! Protocol of the enhance module: the state it shows and the commands that
//! change it. Other modules reach it only through what is declared here.

#[bern::protocol(namespace = "enhance", version = 1)]
pub mod enhance {
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

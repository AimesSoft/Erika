//! Protocol of the remote module: remote access, as NipaPlay offers it, on
//! the port the WebUI is served from. NipaPlay clients find this device on the
//! network and play what its library holds; a browser opens the WebUI.

#[bern::protocol(namespace = "remote", version = 1)]
pub mod remote {
    use bern::prelude::*;

    /// An address this device answers on.
    #[value]
    pub struct Address {
        #[content]
        pub url: String,
    }

    /// A device that controls playback here, or asks to.
    #[value]
    pub struct Device {
        pub key: String,
        #[content]
        pub name: String,
        #[content]
        pub platform: String,
        #[content]
        pub address: String,
    }

    #[state]
    pub struct State {
        /// This build carries the WebUI, so it can serve.
        pub available: bool,
        pub enabled: bool,
        pub running: bool,
        pub port: i64,
        pub addresses: Vec<Address>,
        /// Why it could not listen.
        #[content]
        pub error: String,
        /// Other devices may control playback here.
        pub receiver: bool,
        /// Who is waiting for an answer.
        pub asking: Option<Device>,
        pub trusted: Vec<Device>,
        pub has_trusted: bool,
    }

    #[commands]
    pub trait Commands {
        fn set_enabled(&mut self, enabled: bool) -> Result<(), CommandError>;
        #[errors("invalid_port")]
        fn set_port(&mut self, port: String) -> Result<(), CommandError>;
        fn set_receiver(&mut self, enabled: bool) -> Result<(), CommandError>;
        fn copy_address(&mut self, url: String) -> Result<(), CommandError>;
        /// Answer the device that is asking.
        fn answer(&mut self, allow: bool, trust: bool) -> Result<(), CommandError>;
        fn forget_device(&mut self, key: String) -> Result<(), CommandError>;
        /// May this device control playback? `client` is the JSON of a
        /// [`Device`]; `ask` puts the question to the user. Answers
        /// `authorized`, `pending`, `denied` or `required`.
        fn authorize(&mut self, client: String, ask: bool) -> Result<String, CommandError>;
    }
}

# The danmaku module

| Directory | What it holds | Depends on |
| --- | --- | --- |
| `protocol/` | The module's contract: its state, commands, queries and events | `bern` and the protocols of the modules in `depends` |
| `core/` | The engine: plain Rust, no windows | this protocol and the protocols of the modules in `depends` |
| `ui/` | Components the application places in its slots and page slots | this protocol |

- Test the engine on its own: `cargo test -p erika-danmaku-core`.
- Reach another module only through what it declares: `other::calls::…` to run its commands, `other::current_state()` to read its state, `#[on(other::event)]` to answer its events. List it in `depends` in `Bern.toml`.
- Emit this module's events with `danmaku::emit::…`.

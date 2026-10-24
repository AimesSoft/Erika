# The library module

| Directory | What it holds | Depends on |
| --- | --- | --- |
| `protocol/` | The module's contract: its state, commands, queries and events | `bern` and the protocols of the modules in `depends` |
| `core/` | The engine: plain Rust, no windows | this protocol and the protocols of the modules in `depends` |
| `ui/` | Components the application places in its slots and page slots | this protocol |

- Test the engine on its own: `cargo test -p erika-library-core`.
- Reach another module only through what it declares: `other::calls::…` to run its commands, `other::current_state()` to read its state, `#[on(other::event)]` to answer its events. List it in `depends` in `Bern.toml`.
- Emit this module's events with `library::emit::…`.

The Sources page's **Mount** menu offers a local folder or **NipaPlay shared
media**. Local folders initially hold anime; their content type can be changed
from the source's More menu. The NipaPlay dialog takes an optional name and an
HTTP(S) address, using port 1180 for local addresses without an explicit port.
It reads `/api/media/local/share/animes` and each show's episodes before saving
the source and its playable files. Refresh failures preserve the previous
snapshot; cancelled connections do not save a source. Shared sources refresh
at startup and from More, and can be removed like local folders.

**dandanplay shared media** connects to the remote access of dandanplay's PC
client (`erika-dandanplay`'s `remote` module, after NipaPlay's
`DandanplayRemoteService`). The dialog takes an optional name, the address the
client shows (port 80 unless changed there) and an API key, needed only when
the client turned key checking on: `/api/v1/welcome` says so, and the key goes
with `/api/v1/library` as `Authorization: Bearer` and with the stream and
thumbnail addresses as `?token=`. The key is kept in the `setting` table under
`library.dandanplay.<source>.token`, and only when the client asks for one.
The client's matches (`AnimeId`, `EpisodeId`) put its videos on the wall; the
videos it did not match wait among those not recognized, with its fingerprint
(`Hash`, `Size`) to match them by. Shows dandanplay cannot be asked about take
the client's thumbnail as their poster until it can. The source opens as its
shows, with the videos not matched beside them; its More menu refreshes,
edits (empty fields keep what the source has) or removes it. Played without
a dandanplay account, an episode takes its danmaku from the client
(`/api/v1/comment/id/<id>`). Subtitles beside the client's videos and its
remote-control and download APIs are not used.

Shared-media requests run on native hosts, including the WebUI host. The pure
browser/WASM build reports that a native host is required.

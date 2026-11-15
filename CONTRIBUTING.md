# Working on ctfdc

Start with [src/main.rs](src/main.rs) and [commands::run](src/commands/mod.rs).
The binary parses arguments, dispatches one command, and resolves credentials
only when that command needs a server. Demo views, completions, and setup can
run without an existing configuration.

## Find the code

| Concern | Start here |
| --- | --- |
| Command names, flags, aliases, and help | [config/cli.rs](src/config/cli.rs) |
| Configuration types and errors | [config.rs](src/config.rs) |
| Config discovery and precedence | [config/workspace.rs](src/config/workspace.rs) |
| Scoring-start parsing | [config/time.rs](src/config/time.rs) |
| API methods used by commands | [client.rs](src/client.rs) |
| GET retries and attachment streaming | [client/transport.rs](src/client/transport.rs) |
| CTFd response parsing and diagnostics | [client/response.rs](src/client/response.rs) |
| rCTF conversion to the shared models | [client/rctf.rs](src/client/rctf.rs) |
| rCTF JSON types and authentication | [client/rctf/](src/client/rctf) |
| Shared API data types | [models.rs](src/models.rs) |
| Command-specific behavior and output | [commands/](src/commands) |
| Batch submissions and challenge lookup | [commands/submit/](src/commands/submit) |
| Shared download execution | [workspace/download.rs](src/workspace/download.rs) |
| Attachment names, collisions, and manifests | [workspace/attachments.rs](src/workspace/attachments.rs) |
| Downloaded README and metadata updates | [workspace/metadata.rs](src/workspace/metadata.rs) |
| Safe filesystem names and paths | [fs_util.rs](src/fs_util.rs) |
| Live board startup and refresh loop | [board/mod.rs](src/board/mod.rs) |
| Live and demo data sources | [board/source.rs](src/board/source.rs) |
| Background downloads, details, and submissions | [board/actions.rs](src/board/actions.rs) |
| Scoring-window ranking and tie breaking | [board/standings.rs](src/board/standings.rs) |
| Board state, updates, and selections | [board/model.rs](src/board/model.rs) |
| Movement, solve feed, notices, and derived plans | [board/model/](src/board/model) |
| Comparison, difficulty, timing, targets, and route search | [board/analysis/](src/board/analysis) |
| Terminal input and rendering | [tui/](src/tui) |
| HTTP routes and browser snapshot format | [web/](src/web) |
| Browser rendering and styles | [assets/web/](assets/web) |

## Follow a request

A normal command follows `main → commands → client → platform API`.
The client converts both CTFd and rCTF responses into the types in `models.rs`.
Commands should not need platform-specific response handling.

Downloads follow `commands::download → workspace::download → client`.
The workspace module owns the saved files, attachment manifest, and metadata.
The live board calls the same implementation through `board::actions`.
Keep shared operations here rather than importing command handlers into the board.

For live data, `board::source` produces a snapshot, the refresh loop sends an
`Update`, and `BoardModel::apply` updates the model. Its history and planning
modules derive the feed, score series, comparisons, and targets. The terminal
and browser read the same model; selection and layout remain in each frontend.
The browser serializes that model through `web::snapshot` and receives updates
over server-sent events.

## Preserve the contracts

- Credentials come from flags/environment first, then the three workspace
  config locations. URL and token can be filled from different files; optional
  board settings come from the nearest file. Preserve that distinction.
- Keep platform differences in the client adapter. rCTF string IDs must map to
  the same numeric IDs across runs, and its public attachments receive no token.
- Only idempotent GETs use transport retries. Submission rate-limit retries
  belong to the submit command; live submissions report the response immediately.
- Download metadata records completed attachments. Partial files use a temporary
  name, and a repeated download must preserve the user's notes and solve scripts.
- Backend changes must preserve the model and snapshot values used by both live
  views. Rendering, interaction, and style changes are separate work.

## Verify a change

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Integration tests in [tests/](tests) invoke the binary against local `wiremock`
servers; they need permission to bind loopback ports, not live CTF credentials.
[tests/common](tests/common/mod.rs) creates CLI processes with the connection
environment cleared. Set environment values explicitly in tests that need them.
Keep the mocked responses next to the scenarios they explain.

Use the suite matching the behavior while iterating, for example
`cargo test --test download` or `cargo test board::analysis`. Existing terminal
checks live in `src/tui/snapshot_tests.rs`; browser API checks live in
`tests/web.rs`. Run the complete checks before submitting a change.

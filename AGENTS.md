# Repository Guidelines

## Project Structure & Module Organization

This is a Rust CLI crate that builds the `ctfdc` binary from `src/main.rs`. Core API logic lives in `src/client.rs`, data types in `src/models.rs`, configuration resolution in `src/config.rs`, filesystem helpers in `src/fs_util.rs`, and command implementations under `src/commands/`. Integration and command behavior tests are in `tests/` and use mocked CTFd API responses.

## Build, Test, and Development Commands

- `cargo build` compiles the project.
- `cargo run -- <command>` runs the CLI locally, for example `cargo run -- status`.
- `cargo install --path .` installs the `ctfdc` binary from this checkout.
- `cargo fmt --check` verifies Rust formatting.
- `cargo test` runs the full test suite.
- `cargo clippy --all-targets --all-features -- -D warnings` runs lint checks with warnings treated as errors.

## Coding Style & Naming Conventions

Use standard Rust formatting via `rustfmt`; do not hand-align code against formatter output. Prefer small modules with command-specific logic in `src/commands/<name>.rs`. Use `snake_case` for functions, modules, and variables; `PascalCase` for structs, enums, and traits; and clear CLI-facing names that match CTFd concepts. Keep error handling explicit with `anyhow` or typed errors where existing patterns use them.

## Testing Guidelines

Add or update integration tests in `tests/` for CLI behavior and API compatibility. Existing tests use `assert_cmd`, `predicates`, `tempfile`, and `wiremock`; follow those patterns instead of hitting real CTFd servers. Name test files after the command or feature they cover, such as `tests/status.rs` or `tests/download.rs`. Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets --all-features -- -D warnings` before submitting changes.

## Commit & Pull Request Guidelines

Commit history follows Conventional Commit-style prefixes such as `feat:`, `fix:`, and `docs:` with concise imperative summaries. Pull requests should describe the change, list validation commands run, and mention any CTFd API behavior affected. Include linked issues when available and terminal output examples for user-visible CLI changes.

## Security & Configuration Tips

CTFd credentials are supplied through `--url`, `--token`, `CTFD_URL`, `CTFD_TOKEN`, or a `ctfd.toml` written by `ctfdc init` (searched in `.`, `..`, `../..`). Never commit real tokens, print tokens in logs, or add tests that require live credentials. Prefer mocked HTTP interactions for repeatable validation.

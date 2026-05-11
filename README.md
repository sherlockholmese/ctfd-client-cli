# CTFd Client CLI

`ctfd-client` is a Rust command-line client for interacting with a CTFd instance using API token authentication. It can show the leaderboard, list challenge status, download visible challenges, and submit flags.

## Install / Build

Build from the repository root:

```sh
cargo build
```

Run without installing:

```sh
cargo run -- <command>
```

Install the binary locally:

```sh
cargo install --path .
```

## Configuration

All commands require a CTFd base URL and API token. Provide them with flags or environment variables:

```sh
export CTFD_URL="https://ctfd.example.com"
export CTFD_TOKEN="your-api-token"
```

Equivalent flags are available on every command:

```sh
ctfd-client --url https://ctfd.example.com --token your-api-token status
```

Flags override environment variables. Tokens are used only for API authentication and should not be printed or logged.

## Commands

### Leaderboard

Display the CTFd scoreboard:

```sh
ctfd-client leaderboard
ctfd-client --url https://ctfd.example.com --token "$CTFD_TOKEN" leaderboard
```

### Status

Show visible challenges grouped by category, with solved state:

```sh
ctfd-client status
```

### Download

Download all visible challenges into the current directory:

```sh
ctfd-client download
```

For each challenge, the CLI creates a sanitized path:

```text
<category>/<challenge-name>/
  README.md
  metadata.json
  <attachments>
```

`README.md` contains the challenge title, category, points, optional type and connection information, and description. `metadata.json` contains challenge metadata such as ID, name, category, value, description, solved state, type, state, connection info, files, tags, and max attempts when available. Attachment filenames are sanitized and collisions are disambiguated.

### Submit

Submit a flag using the numeric CTFd challenge ID:

```sh
ctfd-client submit 123 'flag{example}'
```

Correct and already-solved submissions exit successfully. Incorrect submissions exit non-zero.

## Test / Validation

Run the standard checks:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The test suite uses mocked CTFd API responses via `wiremock`.

# CTFd Client CLI

`ctfd-client` is a command-line client for [CTFd](https://ctfd.io) that keeps you in the terminal during a CTF: download every challenge with its files, see what's left, and submit flags straight from the challenge folder.

## Quick start

```sh
cargo install --path .

mkdir myctf && cd myctf
ctfd-client init https://ctf.example.com   # asks for your API token, checks it, saves ./ctfd.toml
ctfd-client dl                             # download every challenge
ctfd-client ls -u                          # what's left to solve, with IDs

cd Web/Baby\ Login
ctfd-client s 'flag{...}'                  # submit; the challenge is detected from the folder
```

Running `ctfd-client` with no arguments prints this quick start, and every command has examples under `-h`.

| Command | Aliases | What it does |
| --- | --- | --- |
| `init [URL]` | `login` | Verify and save URL + token for this folder |
| `status` | `ls`, `st` | Challenges by category with IDs, solved state and progress |
| `download` | `dl` | Download challenges and attachments (safe to re-run) |
| `submit` | `s` | Submit flags by folder, ID, or name |
| `leaderboard` | `lb`, `scoreboard` | Scoreboard |
| `completions <SHELL>` | | Shell completion script |

## Setup

Get an API token in CTFd under **Settings → Access Tokens**, then run `init` in the folder you want to use for the CTF:

```sh
ctfd-client init https://ctf.example.com
```

`init` accepts the URL as pasted from the browser (`ctf.example.com/challenges` works). It prompts for the token without echoing it, checks the token against the server, and writes `ctfd.toml` readable only by you. If the folder is a git checkout, it also adds `ctfd.toml` to `.gitignore`. To set things up from a script, pass `--token` (or set `CTFD_TOKEN`); `--no-verify` skips the server check.

Every command looks for `ctfd.toml` in the current folder, then `..`, then `../..`, so it works from the workspace root, a category folder, or a challenge folder. Flags and environment variables override the file:

```sh
export CTFD_URL="https://ctfd.example.com"
export CTFD_TOKEN="your-api-token"
ctfd-client --url https://ctfd.example.com --token "$CTFD_TOKEN" status
```

Precedence: `--url`/`--token` > `CTFD_URL`/`CTFD_TOKEN` > nearest `ctfd.toml`. Tokens are never printed or logged.

Tab completion means fewer things to remember:

```sh
ctfd-client completions bash > ~/.local/share/bash-completion/completions/ctfd-client
ctfd-client completions zsh  > ~/.zfunc/_ctfd-client
ctfd-client completions fish > ~/.config/fish/completions/ctfd-client.fish
```

## Status

```sh
ctfd-client ls            # everything
ctfd-client ls -u         # only unsolved
ctfd-client ls -c web     # one category (repeatable)
```

```text
Solved 1/3 challenges · 100/600 pts

Crypto (0/1)
  [ ] #3   RSA 101 (300 pts)

Web (1/2)
  [✓] #1   Baby Login (100 pts)
  [ ] #12  SQL Injection (200 pts)
```

## Download

```sh
ctfd-client dl
```

Downloads go into the workspace (the folder holding `ctfd.toml`, or the current folder if there is none). Each challenge gets a sanitized path:

```text
<category>/<challenge-name>/
  README.md
  metadata.json
  <attachments>
```

`README.md` contains the challenge title, category, points, optional type and connection information, and description. `metadata.json` contains challenge metadata such as ID, name, category, value, description, solved state, type, state, connection info, files, tags, max attempts, and the attachments saved locally. Attachment filenames are sanitized and collisions are disambiguated.

Downloads are built to be re-run throughout a CTF:

- **Incremental.** Attachments already on disk from a previous run are skipped, so re-running after new challenges are released only fetches what's new. Newly seen challenges are marked `[new]`. An attachment that was deleted locally, or replaced on the server with a new file URL, is fetched again.
- **Your work stays put.** `README.md` and `metadata.json` are refreshed, but other files in a challenge directory (solve scripts, notes, extracted files) are never overwritten. If a new attachment has the same name as one of your files, it is saved as `name_1.ext` instead.
- **Parallel.** Several challenges download at once (8 by default, `-j` to change it).
- **Resilient.** Requests that fail with connection errors, timeouts, or HTTP 429/502/503/504 are retried with backoff. Attachments are streamed to a temporary `.part` file and renamed only when complete, so an interrupted run never leaves a truncated file behind. If a challenge fails, the rest still download and the command exits non-zero.

Options:

| Option | Description |
| --- | --- |
| `[CHALLENGE]...` | Only these challenges: numeric IDs or case-insensitive name substrings |
| `-c, --category <CATEGORY>` | Only this category, case-insensitive (repeatable) |
| `-u, --unsolved` | Skip challenges you have already solved |
| `-o, --output <DIR>` | Download into `DIR` instead of the workspace |
| `-j, --jobs <N>` | Number of challenges to download in parallel (1–64, default 8) |
| `-f, --force` | Re-download attachments even if they already exist |

```sh
ctfd-client dl -u -c pwn -c rev       # only unsolved pwn and rev
ctfd-client dl 42 baby                # challenge 42 plus any name containing "baby"
ctfd-client dl -f "Baby Login"        # refetch one challenge's handouts
```

## Submit

```sh
ctfd-client s 'flag{...}'                      # inside a challenge folder: challenge auto-detected
ctfd-client s 42 'flag{...}'                   # by ID
ctfd-client s 'baby login' 'flag{...}'         # by name, a unique part of it, or Category/Name
```

From a challenge folder, the challenge is read from `metadata.json` (or matched from the `Category/Name` path). A correct flag also marks that `metadata.json` as solved.

Feed it exploit output instead of copying and pasting:

```sh
./solve.py | ctfd-client s - -r 'flag\{[^}]*\}'        # first regex match on stdin
ctfd-client s 42 "$(./solve.py)" -r 'CTF\{.*?\}'       # --regex works on arguments too
echo 'flag{x}' | ctfd-client s 42 -                    # single line on stdin, trimmed
```

Submit several at once with `-F` (`-` reads stdin). Put one entry per line, as `CHALLENGE<TAB>FLAG` or `ID FLAG`; `#` comments are ignored:

```text
# flags.txt
12 flag{first}
Baby Login	flag{second}
```

```sh
ctfd-client s -F flags.txt
```

Each line is reported with its outcome, followed by a summary.

Exit codes: `0` correct or already solved, `2` incorrect, `1` errors (network, unknown challenge, CTF paused). CTFd's "you're submitting too fast" responses are waited out and retried (`--wait SECS`, default 10, `0` to disable). An out-of-attempts response is shown as a rejection that includes the server's message.

## Leaderboard

```sh
ctfd-client lb
```

## Test / Validation

Run the standard checks:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The test suite uses mocked CTFd API responses via `wiremock`.

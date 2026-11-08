# CTFd Client CLI

`ctfdc` is a command-line client for [CTFd](https://ctfd.io) that keeps you in the terminal during a CTF: download every challenge with its files, see what's left, and submit flags straight from the challenge folder.

## Quick start

```sh
cargo install --path .

mkdir myctf && cd myctf
ctfdc init https://ctf.example.com   # asks for your API token, checks it, saves ./ctfd.toml
ctfdc dl                             # download every challenge
ctfdc ls -u                          # what's left to solve, with IDs

cd Web/Baby\ Login
ctfdc s 'flag{...}'                  # submit; the challenge is detected from the folder
```

Running `ctfdc` with no arguments prints this quick start, and every command has examples under `-h`.

| Command | Aliases | What it does |
| --- | --- | --- |
| `init [URL]` | `login` | Verify and save URL + token for this folder |
| `status` | `ls`, `st` | Challenges by category with IDs, solved state and progress |
| `download` | `dl` | Download challenges and attachments (safe to re-run) |
| `submit` | `s` | Submit flags by folder, ID, or name |
| `leaderboard` | `lb`, `scoreboard` | Live scoreboard with graph and rival analysis (`--web` for the browser, `--print` for text) |
| `completions <SHELL>` | | Shell completion script |

## Setup

Get an API token in CTFd under **Settings -> Access Tokens**, then run `init` in the folder you want to use for the CTF:

```sh
ctfdc init https://ctf.example.com
```

`init` accepts the URL as pasted from the browser (`ctf.example.com/challenges` works). It prompts for the token without echoing it, checks the token against the server, and writes `ctfd.toml` readable only by you. If the folder is a git checkout, it also adds `ctfd.toml` to `.gitignore`. To set things up from a script, pass `--token` (or set `CTFD_TOKEN`); `--no-verify` skips the server check.

Every command looks for `ctfd.toml` in the current folder, then `..`, then `../..`, so it works from the workspace root, a category folder, or a challenge folder. Flags and environment variables override the file:

```sh
export CTFD_URL="https://ctfd.example.com"
export CTFD_TOKEN="your-api-token"
ctfdc --url https://ctfd.example.com --token "$CTFD_TOKEN" status
```

Precedence: `--url`/`--token` > `CTFD_URL`/`CTFD_TOKEN` > nearest `ctfd.toml`. Tokens are never printed or logged.

### rCTF

`ctfdc` also works with [rCTF](https://rctf.osec.io) (v2, and v1 servers). Paste the login link from your rCTF profile page:

```sh
ctfdc init 'https://ctf.example.com/login?token=...'
```

`init` recognises rCTF on its own (by its public client config) and saves `platform = "rctf"` with your team token in `ctfd.toml`; `--platform rctf` (or `CTFD_PLATFORM`) forces it. Every command then works the same: `ls`, `dl`, `s`, and `lb` in the terminal or `--web`. rCTF challenge and team IDs are strings, so `ctfdc` shows a stable number for each (submit by folder or name as usual). Attachments are downloaded without your token, since rCTF serves them from public storage.

Tab completion means fewer things to remember:

```sh
ctfdc completions bash > ~/.local/share/bash-completion/completions/ctfdc
ctfdc completions zsh  > ~/.zfunc/_ctfdc
ctfdc completions fish > ~/.config/fish/completions/ctfdc.fish
```

## Status

```sh
ctfdc ls            # everything
ctfdc ls -u         # only unsolved
ctfdc ls -c web     # one category (repeatable)
```

```text
Solved 1/3 challenges, 100/600 pts

Crypto (0/1)
  [ ] #3   RSA 101 (300 pts)

Web (1/2)
  [✓] #1   Baby Login (100 pts)
  [ ] #12  SQL Injection (200 pts)
```

## Download

```sh
ctfdc dl
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
ctfdc dl -u -c pwn -c rev       # only unsolved pwn and rev
ctfdc dl 42 baby                # challenge 42 plus any name containing "baby"
ctfdc dl -f "Baby Login"        # refetch one challenge's handouts
```

## Submit

```sh
ctfdc s 'flag{...}'                      # inside a challenge folder: challenge auto-detected
ctfdc s 42 'flag{...}'                   # by ID
ctfdc s 'baby login' 'flag{...}'         # by name, a unique part of it, or Category/Name
```

From a challenge folder, the challenge is read from `metadata.json` (or matched from the `Category/Name` path). A correct flag also marks that `metadata.json` as solved.

Feed it exploit output instead of copying and pasting:

```sh
./solve.py | ctfdc s - -r 'flag\{[^}]*\}'        # first regex match on stdin
ctfdc s 42 "$(./solve.py)" -r 'CTF\{.*?\}'       # --regex works on arguments too
echo 'flag{x}' | ctfdc s 42 -                    # single line on stdin, trimmed
```

Submit several at once with `-F` (`-` reads stdin). Put one entry per line, as `CHALLENGE<TAB>FLAG` or `ID FLAG`; `#` comments are ignored:

```text
# flags.txt
12 flag{first}
Baby Login	flag{second}
```

```sh
ctfdc s -F flags.txt
```

Each line is reported with its outcome, followed by a summary.

Exit codes: `0` correct or already solved, `2` incorrect, `1` errors (network, unknown challenge, CTF paused). CTFd's "you're submitting too fast" responses are waited out and retried (`--wait SECS`, default 10, `0` to disable). An out-of-attempts response is shown as a rejection that includes the server's message.

## Scoreboard

```sh
ctfdc lb                 # live full-screen scoreboard
ctfdc lb --print         # print once as text (-p; also what you get when piped)
ctfdc lb --demo          # try the live view on fictional, simulated data
```

The live view updates every 15 seconds (`-n SECS`), keeps the last good data on screen if an update fails, and has three tabs (`Tab` switches):

- **Board**: standings with points behind the team above and each team's rank/score movement over the last 15 minutes, a score-over-time graph of the top 10 (`--top N`; graphed teams' names are drawn in their line color), and the latest solves as they happen. `Enter` adds any team to the graph, `/` finds a team, `m` jumps to you, `l` switches layout.
- **Analysis**: you against a rival (the team above you by default; `<-`/`->` changes it). The first two lines say where you stand and what to do: how far behind or ahead you are, your margin to the qualifying line, and how many catch-up challenges it takes to pass them. Below that:
  - **Catch up**: challenges the rival solved and you haven't, most-solved first, with a running total and a line where you'd pass them.
  - **Open for both**: challenges neither of you has solved, ranked by how many of the top teams solved them.
  - **Head to head**: score, solves, points in the last hour / 3 hours, time since the last solve.
  - **Points by category**: a radar of the share of each category's points you and the rival hold, with the raw numbers on hover (web) and the biggest gaps underneath (terminal).
  - **Your edge**: challenges you have that they don't.
- **Plan**: you against the whole board, for deciding what to work on next (`3` or `Tab` again).
  - **Best targets**: every challenge you haven't solved, ranked by expected points (points × the share of teams that solved it), with a running total and the rank you would reach, if nobody else scores.
  - **Path to the top**: the top 3, the teams just above you and the two just behind, each with the gap, their last hour, and how many of your best targets it takes to pass them.
  - **Where to focus**: categories by the expected points still open.

Press `?` in the live view for every key.

### In the browser

```sh
ctfdc lb --web                       # serve the live scoreboard and open it in your browser (-w)
ctfdc lb --web --demo                # the same on fictional data
ctfdc lb --web --port 8080 --no-open # fixed port, print the link instead of opening it
```

The web view shows the same Board, Analysis and Plan as the terminal, with an interactive score graph (hover for everyone's score at that moment, ranges from `All` down to the last `30m`, click a legend entry to hide a line). Hovering a team in the standings highlights its line. `Compare` on any row opens Analysis against that team, and the pin button adds a team outside the top N to the graph. It follows your system's light/dark setting, and the sun/moon button switches it. `--since`, `--cutoff`, `--top` and `-n` work as in the terminal.

The server listens on `127.0.0.1` only (`--bind ADDR` to change it; ctfdc warns when it isn't a loopback address). It makes every CTFd request itself, so your API token never reaches the browser. Each run prints a new link ending in `#k=<key>`, and the page only works with that key, so other users on the machine and other sites in your browser can't read or change the board. To view it from another machine, forward the port over SSH (`ssh -L 8080:127.0.0.1:8080 host`, then run with `--port 8080`). Charts use the vendored [Apache ECharts](https://echarts.apache.org/) 5.6.0 (`assets/web/vendor`, Apache-2.0), so the page needs no internet access.

### Qualifying line

Set how many places qualify in `ctfd.toml` and the scoreboard draws the line under that place (live view, Analysis, `--print`, and `--demo`):

```toml
cutoff = 20
```

`ctfdc init <url> --cutoff 20` writes it for you, and `ctfdc lb --cutoff N` overrides it for one run. `qualify = 20` is accepted as a synonym.

### Counting points from a set time

To rank teams only on points scored from a given moment (for example, a second day or a finals round), set `since` in `ctfd.toml`. Solves and awards before that time are ignored and the scoreboard is re-ranked (live view, graph, Analysis and `--print`):

```toml
since = 2026-11-07T09:00:00+08:00   # without an offset it is local time
# since = "2026-11-07 09:00"         # strings and Unix seconds work too
```

`ctfdc lb --since '2026-11-07 09:00'` overrides it for one run. Ties go to whoever reached the score first, as on CTFd. The re-ranked board is computed from `/api/v1/scoreboard/top/N`, so only accounts CTFd lists on the scoreboard are included.

## Test / Validation

Run the standard checks:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The test suite uses mocked CTFd API responses via `wiremock`.

use crate::client::{CtfdClient, CtfdError, DenyKind, detect_rctf};
use crate::config::{GlobalArgs, InitArgs, Platform, ResolvedConfig};
use std::io::{IsTerminal, Write};
use std::path::Path;

const CONFIG_FILE: &str = "ctfd.toml";

/// Set up the current folder as a CTF workspace.
///
/// Collects the URL (argument, `--url`/`CTFD_URL`, existing `ctfd.toml`, or
/// a prompt) and token (`--token`/`CTFD_TOKEN` or a hidden prompt),
/// verifies the token with `GET /api/v1/users/me`, and writes `ctfd.toml`
/// (with `platform = "rctf"` when the server is rCTF; an rCTF login link
/// `https://ctf/login?token=...` supplies both URL and team token)
/// readable only by the current user.  When the folder is a git checkout,
/// `ctfd.toml` is added to `.gitignore` so the token is not committed.
pub async fn run(global: &GlobalArgs, args: InitArgs) -> anyhow::Result<()> {
    let path = Path::new(CONFIG_FILE);
    let existing = read_existing(path);
    let cutoff = args.cutoff.or(existing.cutoff);

    let raw_url = match first_non_empty([args.url.clone(), global.url.clone(), existing.url]) {
        Some(url) => url,
        None => prompt_line("CTF URL or rCTF login link (e.g. https://ctf.example.com): ")?,
    };
    let link_token = login_link_token(&raw_url);
    let url = normalize_url(&raw_url)?;

    let platform = match (global.platform, link_token.is_some()) {
        (Some(p), _) => p,
        (None, true) => Platform::Rctf,
        (None, false) if detect_rctf(&url).await => Platform::Rctf,
        (None, false) => existing.platform.unwrap_or_default(),
    };
    if platform == Platform::Rctf {
        println!("{url} runs rCTF.");
    }

    let token = match first_non_empty([global.token.clone(), link_token]) {
        Some(token) => token,
        None => prompt_token(&url, platform, existing.token)?,
    };

    let config = ResolvedConfig {
        base_url: url.clone(),
        api_token: token.clone(),
        platform,
    };

    if args.no_verify {
        println!("Skipping verification (--no-verify).");
    } else {
        match CtfdClient::new(&config).get_me().await {
            Ok(me) => println!("Logged in to {url} as {}.", me.name),
            Err(CtfdError::Denied { kind, message }) if kind.token_accepted() => {
                let first = message.lines().next().unwrap_or_default();
                println!("Token accepted by {url} ({first})");
            }
            Err(CtfdError::Denied {
                kind: DenyKind::BadToken,
                ..
            }) => match platform {
                Platform::Ctfd => anyhow::bail!(
                    "The server rejected this token. Create a new one in CTFd under \
                     Settings -> Access Tokens ({url}/settings) and run init again."
                ),
                Platform::Rctf => anyhow::bail!(
                    "rCTF rejected this team token. Copy the login link from your profile \
                     ({url}/profile) and run `ctfdc init '<link>'` again."
                ),
            },
            Err(e) => anyhow::bail!(
                "Could not verify the token with {url}: {e}\n\
                 Check the URL, or re-run with --no-verify to save it anyway."
            ),
        }
    }

    write_config(path, &url, &token, platform, cutoff)?;
    println!("Saved {CONFIG_FILE} (readable only by you).");
    if let Some(n) = cutoff {
        println!("Qualifying line: top {n}.");
    }

    if ensure_gitignored()? {
        println!("Added {CONFIG_FILE} to .gitignore so your token isn't committed.");
    }

    println!(
        "\nNext steps:\n  \
         ctfdc dl     Download all challenges into this folder\n  \
         ctfdc ls     List challenges with IDs\n  \
         cd <Category>/<Challenge> && ctfdc s 'flag{{...}}'"
    );
    Ok(())
}

fn first_non_empty<const N: usize>(values: [Option<String>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|v| v.trim().to_string())
        .find(|v| !v.is_empty())
}

/// Settings from an existing `ctfd.toml`.
#[derive(Default)]
struct Existing {
    url: Option<String>,
    token: Option<String>,
    cutoff: Option<u32>,
    platform: Option<Platform>,
}

/// Read an existing config file, ignoring any problems.
fn read_existing(path: &Path) -> Existing {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Existing::default();
    };
    let Ok(table) = toml::from_str::<toml::Table>(&content) else {
        return Existing::default();
    };
    let cutoff = table
        .get("cutoff")
        .or_else(|| table.get("qualify"))
        .and_then(|v| v.as_integer())
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0);
    let get = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let platform = match get("platform").as_deref() {
        Some("rctf") => Some(Platform::Rctf),
        Some("ctfd") => Some(Platform::Ctfd),
        _ => None,
    };
    Existing {
        url: get("url"),
        token: get("token"),
        cutoff,
        platform,
    }
}

/// The team token in an rCTF login link (`.../login?token=...`).
fn login_link_token(raw: &str) -> Option<String> {
    let query = raw.split_once('?')?.1;
    let query = query.split('#').next().unwrap_or(query);
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .map(|t| urlencoding::decode(t).map_or_else(|_| t.to_string(), |d| d.into_owned()))
        .filter(|t| !t.trim().is_empty())
}

/// Normalize what people paste: add `https://`, drop trailing slashes and
/// a trailing CTFd page path such as `/challenges` or `/login`.
pub fn normalize_url(raw: &str) -> anyhow::Result<String> {
    let mut url = raw.trim().to_string();
    if url.is_empty() {
        anyhow::bail!("A CTFd URL is required, e.g. ctfdc init https://ctf.example.com");
    }
    if !url.contains("://") {
        url = format!("https://{url}");
    }
    if let Some(cut) = url.find(['?', '#']) {
        url.truncate(cut);
    }
    let mut url = url.trim_end_matches('/').to_string();
    for page in [
        "/challenges",
        "/scoreboard",
        "/login",
        "/register",
        "/settings",
        "/users",
        "/teams",
        "/notifications",
        // rCTF pages.
        "/profile",
        "/scores",
        "/challs",
    ] {
        if let Some(stripped) = url.strip_suffix(page) {
            url = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        anyhow::bail!("The CTFd URL must start with http:// or https://, got: {raw}");
    }
    Ok(url)
}

fn prompt_line(prompt: &str) -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Missing CTFd URL. Usage: ctfdc init <url>");
    }
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn prompt_token(url: &str, platform: Platform, existing: Option<String>) -> anyhow::Result<String> {
    let noun = match platform {
        Platform::Ctfd => "API token",
        Platform::Rctf => "Team token or login link",
    };
    if !std::io::stdin().is_terminal() {
        return existing.ok_or_else(|| {
            anyhow::anyhow!("Missing {noun}. Pass --token <token> or set CTFD_TOKEN.")
        });
    }
    match platform {
        Platform::Ctfd => println!(
            "Create an API token in CTFd under Settings -> Access Tokens ({url}/settings)."
        ),
        Platform::Rctf => {
            println!("Copy your login link (or team token) from your rCTF profile ({url}/profile).")
        }
    }
    let prompt = if existing.is_some() {
        format!("{noun} (input hidden, Enter keeps the current one): ")
    } else {
        format!("{noun} (input hidden): ")
    };
    let entered = rpassword::prompt_password(prompt)?;
    let entered = entered.trim();
    let entered = login_link_token(entered).unwrap_or_else(|| entered.to_string());
    match (entered.as_str(), existing) {
        ("", Some(existing)) => Ok(existing),
        ("", None) => anyhow::bail!("A {noun} is required."),
        (token, _) => Ok(token.to_string()),
    }
}

fn write_config(
    path: &Path,
    url: &str,
    token: &str,
    platform: Platform,
    cutoff: Option<u32>,
) -> anyhow::Result<()> {
    // Keep other settings (e.g. `since`) from an existing, valid file.
    let mut table = std::fs::read_to_string(path)
        .ok()
        .and_then(|c| toml::from_str::<toml::Table>(&c).ok())
        .unwrap_or_default();
    table.remove("qualify");
    table.insert("url".into(), toml::Value::String(url.into()));
    table.insert("token".into(), toml::Value::String(token.into()));
    match platform {
        Platform::Ctfd => table.remove("platform"),
        Platform::Rctf => table.insert(
            "platform".into(),
            toml::Value::String(platform.key().into()),
        ),
    };
    if let Some(n) = cutoff {
        table.insert("cutoff".into(), toml::Value::Integer(i64::from(n)));
    }
    let content = format!(
        "# ctfdc workspace config. Contains your API token: do not share or commit.\n{}",
        toml::to_string(&table)?
    );

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| anyhow::anyhow!("Cannot write {CONFIG_FILE}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Tighten permissions of a pre-existing file too.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Add `ctfd.toml` to `.gitignore` when this folder is a git checkout or
/// already has a `.gitignore`.  Returns whether the file was changed.
fn ensure_gitignored() -> anyhow::Result<bool> {
    let gitignore = Path::new(".gitignore");
    if !gitignore.exists() && !Path::new(".git").exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(gitignore).unwrap_or_default();
    if content
        .lines()
        .any(|l| matches!(l.trim(), "ctfd.toml" | "/ctfd.toml"))
    {
        return Ok(false);
    }
    let mut new = content;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(CONFIG_FILE);
    new.push('\n');
    std::fs::write(gitignore, new)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{login_link_token, normalize_url};

    #[test]
    fn reads_rctf_login_links() {
        assert_eq!(
            login_link_token("https://ctf.example.com/login?token=abc%2Bdef%3D").as_deref(),
            Some("abc+def=")
        );
        assert_eq!(login_link_token("https://ctf.example.com/login"), None);
        assert_eq!(
            normalize_url("https://ctf.example.com/login?token=abc").unwrap(),
            "https://ctf.example.com"
        );
    }

    #[test]
    fn normalizes_pasted_urls() {
        assert_eq!(
            normalize_url("ctf.example.com").unwrap(),
            "https://ctf.example.com"
        );
        assert_eq!(
            normalize_url("https://ctf.example.com/challenges#Web").unwrap(),
            "https://ctf.example.com"
        );
        assert_eq!(
            normalize_url("http://10.0.0.1:8000/").unwrap(),
            "http://10.0.0.1:8000"
        );
        assert_eq!(
            normalize_url("https://host/ctf/login?next=/").unwrap(),
            "https://host/ctf"
        );
        assert!(normalize_url("  ").is_err());
        assert!(normalize_url("ftp://x").is_err());
    }
}

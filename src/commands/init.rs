use crate::client::{CtfdClient, CtfdError};
use crate::config::{GlobalArgs, InitArgs, ResolvedConfig};
use std::io::{IsTerminal, Write};
use std::path::Path;

const CONFIG_FILE: &str = "ctfd.toml";

/// Set up the current folder as a CTF workspace.
///
/// Collects the URL (argument, `--url`/`CTFD_URL`, existing `ctfd.toml`, or
/// a prompt) and token (`--token`/`CTFD_TOKEN` or a hidden prompt),
/// verifies the token with `GET /api/v1/users/me`, and writes `ctfd.toml`
/// readable only by the current user.  When the folder is a git checkout,
/// `ctfd.toml` is added to `.gitignore` so the token is not committed.
pub async fn run(global: &GlobalArgs, args: InitArgs) -> anyhow::Result<()> {
    let path = Path::new(CONFIG_FILE);
    let (existing_url, existing_token) = read_existing(path);

    let url = match first_non_empty([args.url.clone(), global.url.clone(), existing_url]) {
        Some(url) => url,
        None => prompt_line("CTFd URL (e.g. https://ctf.example.com): ")?,
    };
    let url = normalize_url(&url)?;

    let token = match first_non_empty([global.token.clone()]) {
        Some(token) => token,
        None => prompt_token(&url, existing_token)?,
    };

    let config = ResolvedConfig {
        base_url: url.clone(),
        api_token: token.clone(),
    };

    if args.no_verify {
        println!("Skipping verification (--no-verify).");
    } else {
        match CtfdClient::new(&config).get_me().await {
            Ok(me) => println!("Logged in to {url} as {}.", me.name),
            Err(CtfdError::Api(msg)) if msg.starts_with("Unauthorized") => anyhow::bail!(
                "The server rejected this token. Create a new one in CTFd under \
                 Settings → Access Tokens ({url}/settings) and run init again."
            ),
            Err(e) => anyhow::bail!(
                "Could not verify the token with {url}: {e}\n\
                 Check the URL, or re-run with --no-verify to save it anyway."
            ),
        }
    }

    write_config(path, &url, &token)?;
    println!("Saved {CONFIG_FILE} (readable only by you).");

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

/// Read url/token from an existing config file, ignoring any problems.
fn read_existing(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Ok(table) = toml::from_str::<toml::Table>(&content) else {
        return (None, None);
    };
    let get = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    (get("url"), get("token"))
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

fn prompt_token(url: &str, existing: Option<String>) -> anyhow::Result<String> {
    if !std::io::stdin().is_terminal() {
        return existing.ok_or_else(|| {
            anyhow::anyhow!("Missing API token. Pass --token <token> or set CTFD_TOKEN.")
        });
    }
    println!("Create an API token in CTFd under Settings → Access Tokens ({url}/settings).");
    let prompt = if existing.is_some() {
        "API token (input hidden, Enter keeps the current one): "
    } else {
        "API token (input hidden): "
    };
    let entered = rpassword::prompt_password(prompt)?;
    match (entered.trim(), existing) {
        ("", Some(existing)) => Ok(existing),
        ("", None) => anyhow::bail!("An API token is required."),
        (token, _) => Ok(token.to_string()),
    }
}

fn write_config(path: &Path, url: &str, token: &str) -> anyhow::Result<()> {
    let mut table = toml::Table::new();
    table.insert("url".into(), toml::Value::String(url.into()));
    table.insert("token".into(), toml::Value::String(token.into()));
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
    use super::normalize_url;

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

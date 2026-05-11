use crate::config::ResolvedConfig;

pub fn run(config: ResolvedConfig, challenge_id: &str, _flag: &str) -> anyhow::Result<()> {
    println!(
        "submit: would submit flag for challenge {} to {} (token present: {})",
        challenge_id,
        config.base_url,
        !config.api_token.is_empty()
    );
    Ok(())
}

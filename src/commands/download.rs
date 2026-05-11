use crate::config::ResolvedConfig;

pub fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    println!(
        "download: would fetch from {} (token present: {})",
        config.base_url,
        !config.api_token.is_empty()
    );
    Ok(())
}

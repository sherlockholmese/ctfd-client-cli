use crate::client::CtfdClient;
use crate::config::ResolvedConfig;
use std::collections::BTreeMap;

/// Fetch and display challenge status grouped by category.
///
/// Output is deterministic: categories are sorted alphabetically and
/// challenges within a category are displayed in the order returned by
/// the API.  Solved challenges are marked with `[✓]` and unsolved with
/// `[ ]`.
pub async fn run(config: ResolvedConfig) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let challenges = client.get_challenges().await?;

    if challenges.is_empty() {
        println!("No challenges available.");
        return Ok(());
    }

    // Group challenges by category, preserving insertion order within each
    // group.  Use a BTreeMap so categories are sorted alphabetically.
    let mut categories: BTreeMap<&str, Vec<&crate::models::Challenge>> = BTreeMap::new();
    for ch in &challenges {
        categories.entry(ch.category.as_str()).or_default().push(ch);
    }

    let mut first_category = true;
    for cat_challenges in categories.values() {
        // Blank line between categories (but not before the first one).
        if !first_category {
            println!();
        }
        first_category = false;

        // Use the category name from the first challenge in the group.
        let category_name = &cat_challenges[0].category;
        println!("{category_name}");

        for ch in cat_challenges {
            let marker = if ch.solved_by_me { "[✓]" } else { "[ ]" };
            println!(
                "  {marker} {name} ({value} pts)",
                name = ch.name,
                value = ch.value
            );
        }
    }

    Ok(())
}

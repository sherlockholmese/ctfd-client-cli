use crate::client::CtfdClient;
use crate::config::{ResolvedConfig, StatusArgs};
use crate::models::Challenge;
use std::collections::BTreeMap;

/// Fetch and display challenge status grouped by category.
///
/// Starts with an overall progress line, then lists categories sorted
/// alphabetically (with per-category progress) and challenges in API
/// order.  Each challenge shows its ID (for `submit`/`download`), solved
/// marker (`[✓]` / `[ ]`), name and points.  `--unsolved` and
/// `--category` filter the listing; the progress line always covers
/// every visible challenge.
pub async fn run(config: ResolvedConfig, args: StatusArgs) -> anyhow::Result<()> {
    let client = CtfdClient::new(&config);
    let challenges = client.get_challenges().await?;

    if challenges.is_empty() {
        println!("No challenges available.");
        return Ok(());
    }

    let solved: Vec<&Challenge> = challenges.iter().filter(|c| c.solved_by_me).collect();
    let total_points: i64 = challenges.iter().map(|c| c.value).sum();
    let solved_points: i64 = solved.iter().map(|c| c.value).sum();
    println!(
        "Solved {}/{} challenges · {solved_points}/{total_points} pts",
        solved.len(),
        challenges.len()
    );

    let wanted_categories: Vec<String> = args
        .categories
        .iter()
        .map(|c| c.trim().to_lowercase())
        .collect();

    // Group challenges by category, preserving API order within each
    // group.  BTreeMap keeps categories sorted alphabetically.
    let mut categories: BTreeMap<&str, Vec<&Challenge>> = BTreeMap::new();
    for ch in &challenges {
        categories.entry(ch.category.as_str()).or_default().push(ch);
    }

    let id_width = challenges
        .iter()
        .map(|c| c.id.to_string().len())
        .max()
        .unwrap_or(1);

    let mut shown_any = false;
    for (category_name, cat_challenges) in &categories {
        if !wanted_categories.is_empty()
            && !wanted_categories.contains(&category_name.trim().to_lowercase())
        {
            continue;
        }

        let shown: Vec<&&Challenge> = cat_challenges
            .iter()
            .filter(|c| !(args.unsolved && c.solved_by_me))
            .collect();
        if shown.is_empty() {
            continue;
        }
        shown_any = true;

        let cat_solved = cat_challenges.iter().filter(|c| c.solved_by_me).count();
        println!();
        println!("{category_name} ({cat_solved}/{})", cat_challenges.len());

        for ch in shown {
            let marker = if ch.solved_by_me { "[✓]" } else { "[ ]" };
            println!(
                "  {marker} #{id:<id_width$}  {name} ({value} pts)",
                id = ch.id,
                name = ch.name,
                value = ch.value
            );
        }
    }

    if !shown_any {
        println!();
        if args.unsolved && wanted_categories.is_empty() {
            println!("Everything is solved. Nice work!");
        } else {
            println!("No challenges match the given filters.");
        }
    }

    Ok(())
}

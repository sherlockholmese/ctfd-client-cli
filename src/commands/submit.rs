use crate::client::CtfdClient;
use crate::config::ResolvedConfig;
use crate::models::SubmitOutcome;

/// Submit a flag for a challenge.
///
/// Validates that `challenge_id` is numeric before making any network
/// request.  Then sends a POST to `/api/v1/challenges/attempt` with
/// `challenge_id` and `submission` in the JSON body.
///
/// Exit semantics:
/// - Accepted (correct): exits 0, success message on stdout.
/// - Already solved: exits 0, already-solved message on stdout.
/// - Rejected (incorrect): exits non-zero with a submission-outcome
///   message on stderr; not presented as a transport failure.
pub async fn run(config: ResolvedConfig, challenge_id: &str, flag: &str) -> anyhow::Result<()> {
    // Parse the numeric challenge ID before any network access.
    let parsed_id: u64 = challenge_id.parse().map_err(|_| {
        anyhow::anyhow!("Challenge ID must be a numeric value, got: {challenge_id}")
    })?;

    let client = CtfdClient::new(&config);
    let response = client.submit_flag(parsed_id, flag).await?;

    match response.outcome() {
        SubmitOutcome::Accepted => {
            println!("{}", response.user_message());
            Ok(())
        }
        SubmitOutcome::AlreadySolved => {
            println!("{}", response.user_message());
            Ok(())
        }
        SubmitOutcome::Rejected => {
            // Rejected is a submission outcome, not a transport/configuration
            // error.  Print to stderr and exit non-zero.
            eprintln!("{}", response.user_message());
            std::process::exit(2);
        }
    }
}

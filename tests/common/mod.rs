//! Shared CLI process setup. Tests opt in to environment values explicitly.

pub fn ctfd() -> assert_cmd::Command {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("ctfdc"));
    command
        .env_remove("CTFD_URL")
        .env_remove("CTFD_TOKEN")
        .env_remove("CTFD_PLATFORM");
    command
}

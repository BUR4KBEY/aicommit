pub mod ai;
pub mod cli;
pub mod cli_text;
pub mod commands;
pub mod config;
pub mod errors;
pub mod generator;
pub mod git;
pub mod history_store;
pub mod map;
pub mod output;
pub mod prompt;
pub mod token;
pub mod ui;

pub async fn run() -> anyhow::Result<()> {
    cli::run().await
}

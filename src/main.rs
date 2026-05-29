mod api;
mod command;
mod config;
mod prompt;
mod session;
mod tui;

use std::fs;
use tracing::info;

fn init_logging() -> anyhow::Result<()> {
    let _ = fs::create_dir_all(".ncoding");
    let file = fs::File::create(".ncoding/n-coding.log")?;
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(file))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "n_coding=info".into()),
        )
        .init();
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging()?;

    info!("N-coding starting...");

    let cfg = config::loader::load()?;
    info!(
        "Configuration loaded: model={}, thinking={}, max_tokens={}",
        cfg.api.model,
        cfg.thinking.reasoning_effort,
        cfg.api.max_tokens
    );

    let mut terminal = tui::app::init_terminal()?;
    let app = tui::app::App::new(cfg);
    let result = app.run(&mut terminal).await;

    tui::app::restore_terminal(&mut terminal)?;
    info!("N-coding stopped");

    Ok(result?)
}

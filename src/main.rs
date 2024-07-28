mod commands;
mod log;
mod store;

use clap::{Parser, Subcommand};
use std::io::Write;

/// A CLI tool to monitor your AtCoder submission.
#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Login to AtCoder.
    Login,

    /// Monitor your submission in the contest.
    Monitor {
        /// The URL of the contest you want to monitor.
        /// If not specified, the tool will infer the contest URL from the current directory.
        contest_url: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    log::init();

    let result = match args.command {
        Commands::Login => commands::login::main().await,
        Commands::Monitor { contest_url } => {
            let contest_url = contest_url.unwrap_or_else(|| {
                let cwd = std::env::current_dir().unwrap();
                let contest_id = cwd.file_name().unwrap().to_str().unwrap();
                let contest_url = format!("https://atcoder.jp/contests/{}", contest_id);
                info!("Inferred contest URL: {}", contest_url);
                contest_url
            });
            commands::monitor::main(contest_url).await
        }
    };
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    if let Err(err) = result {
        error!("{}", err);
        std::process::exit(1);
    }

    Ok(())
}

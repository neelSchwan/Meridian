use clap::{Parser, Subcommand};
use core_types::{Patch, Tier};
use ingest::{Sampling, fetch_patch_notes, import_notes, ingest_matches};
use riven::RiotApi;
use riven::consts::{Division, PlatformRoute};
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

#[derive(Parser)]
#[command(
    name = "meridian-ingest",
    about = "Collect ranked matches and patch notes into the Meridian database"
)]
struct Cli {
    /// Path to the SQLite database
    #[arg(
        long,
        env = "MERIDIAN_DB",
        default_value = "meridian.db",
        global = true
    )]
    db: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch recent ranked matches of players in one region and tier
    Matches {
        /// Riot API key
        #[arg(long, env = "RGAPI_KEY", hide_env_values = true)]
        api_key: String,
        /// Platform, such as na1 or euw1
        #[arg(long, value_parser = parse_upper::<PlatformRoute>)]
        region: PlatformRoute,
        #[arg(long, default_value = "DIAMOND")]
        tier: Tier,
        /// Division within the tier (I to IV); ignored for Master and above
        #[arg(long, default_value = "I", value_parser = parse_upper::<Division>)]
        division: Division,
        /// Pages of players to read (200 per page below Master)
        #[arg(long, default_value_t = 1)]
        pages: i32,
        #[arg(long, default_value_t = 50)]
        players: usize,
        /// Recent matches to list per player (at most 100)
        #[arg(long, default_value_t = 20)]
        matches_per_player: i32,
        /// Ignore matches older than this many days
        #[arg(long, default_value_t = 14)]
        since_days: i64,
    },

    /// Read a patch's notes and store its champion balance changes
    Notes {
        /// Game version, such as 16.2 (its notes are labelled 26.2)
        patch: Patch,
        /// Read this saved page instead of downloading it
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

/// Riot's enums only parse from their upper-case names, but people type `na1` and `iv`.
fn parse_upper<T: FromStr>(text: &str) -> Result<T, String> {
    text.to_uppercase()
        .parse()
        .map_err(|_| format!("unknown value '{text}'"))
}

#[tokio::main]
async fn main() -> ExitCode {
    // A .env file next to the binary may hold RGAPI_KEY; a missing one is fine.
    dotenv::dotenv().ok();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let conn = engine_api::open_db(&cli.db)?;
    match cli.command {
        Commands::Matches {
            api_key,
            region,
            tier,
            division,
            pages,
            players,
            matches_per_player,
            since_days,
        } => {
            let sampling = Sampling {
                route: region,
                tier,
                division,
                pages,
                max_players: players,
                matches_per_player,
                since_days,
            };
            let summary = ingest_matches(&RiotApi::new(api_key), &conn, &sampling).await?;
            println!("{summary:?}");
        }
        Commands::Notes { patch, file } => {
            let html = match file {
                Some(path) => std::fs::read_to_string(path)?,
                None => fetch_patch_notes(patch).await?,
            };
            let count = import_notes(&conn, patch, &html)?;
            println!("stored {count} balance changes for patch {patch}");
        }
    }
    Ok(())
}

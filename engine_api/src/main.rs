use clap::{Args, Parser, Subcommand, ValueEnum};
use core_types::{Filter, Patch, Role, Tier};
use engine_api::{
    balance_effectiveness, champion_history, list_patches, meta_shifts, open_db, patch_report,
    rankings, render_csv, render_effectiveness, render_history, render_markdown, render_patches,
    render_shifts, render_stats_table,
};
use serde::Serialize;
use serde_json::to_string_pretty;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "meridian",
    about = "Measure how League of Legends balance patches change champion performance"
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

    /// Print JSON instead of text
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Restricts a command to one slice of the data.
#[derive(Args)]
struct FilterArgs {
    /// Platform, such as na1 or euw1
    #[arg(long)]
    region: Option<String>,

    /// Tier of the player each match was found through, such as DIAMOND
    #[arg(long)]
    tier: Option<Tier>,

    /// top, jungle, middle, bottom or support
    #[arg(long)]
    role: Option<Role>,
}

impl FilterArgs {
    fn into_filter(self) -> Filter {
        Filter {
            region: self.region,
            tier: self.tier,
            role: self.role,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Markdown,
    Csv,
}

#[derive(Subcommand)]
enum Commands {
    /// List the patches in the database
    Patches,

    /// Compare a patch with the one before it
    Report {
        /// Game version, such as 16.2
        patch: Patch,
        #[command(flatten)]
        filter: FilterArgs,
        /// Output format (ignored with --json)
        #[arg(long, value_enum, default_value = "markdown")]
        format: Format,
    },

    /// Show how a champion's results changed across patches
    History {
        champion: String,
        #[command(flatten)]
        filter: FilterArgs,
    },

    /// Rank a patch's champions by win rate
    Rankings {
        patch: Patch,
        #[command(flatten)]
        filter: FilterArgs,
        /// How many champions to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// Show which patches moved the meta the most
    Shifts {
        #[command(flatten)]
        filter: FilterArgs,
    },

    /// Show how often buffs and nerfs moved win rates as intended
    Effectiveness {
        #[command(flatten)]
        filter: FilterArgs,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one command and returns what to print.
fn run(cli: Cli) -> Result<String, Box<dyn std::error::Error>> {
    let conn = open_db(&cli.db)?;
    let json = cli.json;
    match cli.command {
        Commands::Patches => {
            let patches = list_patches(&conn)?;
            show(json, &patches, |patches| render_patches(patches))
        }
        Commands::Report {
            patch,
            filter,
            format,
        } => {
            let report = patch_report(&conn, patch, &filter.into_filter())?;
            show(json, &report, |report| match format {
                Format::Markdown => render_markdown(report),
                Format::Csv => render_csv(report),
            })
        }
        Commands::History { champion, filter } => {
            let history = champion_history(&conn, &champion, &filter.into_filter())?;
            show(json, &history, |history| render_history(history))
        }
        Commands::Rankings {
            patch,
            filter,
            limit,
        } => {
            let mut ranked = rankings(&conn, patch, &filter.into_filter())?;
            ranked.truncate(limit);
            show(json, &ranked, |ranked| render_stats_table(ranked))
        }
        Commands::Shifts { filter } => {
            let shifts = meta_shifts(&conn, &filter.into_filter())?;
            show(json, &shifts, |shifts| render_shifts(shifts))
        }
        Commands::Effectiveness { filter } => {
            let result = balance_effectiveness(&conn, &filter.into_filter())?;
            show(json, &result, |result| render_effectiveness(result))
        }
    }
}

/// Formats a result as pretty JSON, or with `text` when JSON was not asked for.
fn show<T: Serialize>(
    json: bool,
    value: &T,
    text: impl FnOnce(&T) -> String,
) -> Result<String, Box<dyn std::error::Error>> {
    if json {
        Ok(to_string_pretty(value)? + "\n")
    } else {
        Ok(text(value))
    }
}

#[cfg(not(debug_assertions))] use human_panic::setup_panic;

#[cfg(debug_assertions)] extern crate better_panic;

use fogrs_utils::error::Result;

use clap::{AppSettings, IntoApp, Parser, Subcommand};
use clap_complete::{
    generate,
    shells::{Bash, Fish, Zsh},
};
use fogrs_core::commands;
use fogrs_utils::app_config::AppConfig;
use fogrs_utils::types::LogLevel;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(
    name = "gdp-router",
    author,
    about,
    long_about = "Rust GDP Router",
    version
)]
#[clap(setting = AppSettings::SubcommandRequired)]
#[clap(global_setting(AppSettings::DeriveDisplayOrder))]
pub struct Cli {
    /// Set a custom config file
    #[clap(short, long, parse(from_os_str), value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Set a custom config file
    #[clap(name = "debug", short, long = "debug", value_name = "DEBUG")]
    pub debug: Option<bool>,

    /// Set Log Level
    #[clap(
        name = "log_level",
        short,
        long = "log-level",
        value_name = "LOG_LEVEL"
    )]
    pub log_level: Option<LogLevel>,

    /// Set Net Interface
    #[clap(
        name = "net_interface",
        short,
        long = "net-interface",
        value_name = "eno1"
    )]
    pub net_interface: Option<String>,

    /// Subcommands
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(
        name = "router",
        about = "Run Router",
        long_about = None,
    )]
    Router,
    #[clap(
        name = "client",
        about = "Run test dtls client",
        long_about = None,
    )]
    Error,
    #[clap(
        name = "completion",
        about = "Generate completion scripts",
        long_about = None,
        )]
    Completion {
        #[clap(subcommand)]
        subcommand: CompletionSubcommand,
    },
    #[clap(
        name = "signal",
        about = "Run Signaling Server",
        long_about = None,
    )]
    Config,
}

#[derive(Subcommand, PartialEq, Debug)]
enum CompletionSubcommand {
    #[clap(about = "generate the autocompletion script for bash")]
    Bash,
    #[clap(about = "generate the autocompletion script for zsh")]
    Zsh,
    #[clap(about = "generate the autocompletion script for fish")]
    Fish,
}

pub fn cli_match() -> Result<()> {
    // Parse the command line arguments
    let cli = Cli::parse();

    // Merge clap config file if the value is set
    AppConfig::merge_config(cli.config.as_deref())?;

    let app = Cli::into_app();

    AppConfig::merge_args(app)?;

    // Execute the subcommand
    match &cli.command {
        Commands::Router => commands::router()?,
        Commands::Error => commands::simulate_error()?,
        Commands::Completion { subcommand } => {
            let mut app = Cli::into_app();
            match subcommand {
                CompletionSubcommand::Bash => {
                    generate(Bash, &mut app, "gdp-router", &mut std::io::stdout());
                }
                CompletionSubcommand::Zsh => {
                    generate(Zsh, &mut app, "gdp-router", &mut std::io::stdout());
                }
                CompletionSubcommand::Fish => {
                    generate(Fish, &mut app, "gdp-router", &mut std::io::stdout());
                }
            }
        }
        Commands::Config => commands::config()?,
    }

    Ok(())
}


/// The main entry point of the application.
fn main() -> Result<()> {
    // Human Panic. Only enabled when *not* debugging.
    #[cfg(not(debug_assertions))]
    {
        setup_panic!();
    }

    // Better Panic. Only enabled *when* debugging.
    #[cfg(debug_assertions)]
    {
        better_panic::Settings::debug()
            .most_recent_first(false)
            .lineno_suffix(true)
            .verbosity(better_panic::Verbosity::Full)
            .install();
    }

    // env_logger::init();

    // Initialize Configuration
    // let include_path = match env::var_os("SGC_CONFIG") {
    //     Some(config_file) => {
    //         config_file.into_string().unwrap()
    //     }
    //     None => "./src/resources/automatic.toml".to_owned(),
    // };

    // println!("Using config file : {}", include_path);
    // let config_contents = fs::read_to_string(include_path).expect("config file not found!");

    // AppConfig::init(Some(&config_contents))?;

    // Match Commands
    cli_match()?;

    Ok(())
}

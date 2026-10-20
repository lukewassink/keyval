use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Path to the database file.
    pub database_path: String,

    pub command_mode: CommandMode,

    pub socket_address: SocketAddr,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CommandMode {
    Repl,
    Tcp,
}

use clap::Parser;
use thiserror::Error;

use crate::config::ConfigError::InvalidOptions;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Invalid startup options: {0}")]
    InvalidOptions(String),
    #[error("Invalid config: {0}")]
    InvalidConfig(#[from] toml::de::Error),
    #[error("Invalid socket address: {0}")]
    InvalidAddress(#[from] std::net::AddrParseError),
}

#[derive(Parser)]
pub struct StartupCli {
    config_path: String,

    /// Path to the database file
    #[arg(long)]
    database_path: Option<String>,

    /// Use the interactive RPL instead of listening on a TCP socket.
    #[arg(long)]
    repl: bool,

    /// TCP socket address in the form: (ip address):(port number)
    #[arg(short, long)]
    socket_address: Option<String>,
}

pub fn build_config() -> Result<Config, ConfigError> {
    // Get comand line options.
    let cli = StartupCli::parse();

    if cli.config_path.is_empty() {
        return Err(InvalidOptions("empty config_path".to_string()));
    }

    let config = std::fs::read_to_string(cli.config_path)?;
    let mut config: Config = toml::from_str(&config)?;

    // Overrride config with cli options if set.
    if let Some(path) = cli.database_path {
        config.database_path = path;
    }
    if cli.repl {
        config.command_mode = CommandMode::Repl;
    }
    if let Some(address) = cli.socket_address {
        config.socket_address = address.parse::<SocketAddr>()?;
    }

    Ok(config)
}

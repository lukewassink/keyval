use crate::Error;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Distinct users to simulate in parallel.
    pub users: usize,
    /// Number of distinct key/val pairs each user should write.
    /// Bear in mind that kv pairs are pregenerated and stored in memory to save rsources while
    /// running.
    pub writes: usize,
    /// Number of read requests each thread should make for a valid key.
    /// If <writes> is zero, this number must be zero too.
    pub reads: usize,
    /// Database address to send requests to.
    pub db_address: SocketAddr,
    /// The file path to write the output data to.
    pub output_path: String,
    /// Minimum length of a random value.
    pub min_val_length: usize,
    /// Maximum length of a random value.
    pub max_val_length: usize,
}

impl Config {
    pub fn total_requests(&self) -> usize {
        (self.reads + self.writes) * self.users
    }
}

#[derive(Debug, Parser)]
struct Cli {
    config_path: String,
}

pub fn get_config_path() -> Result<String, Error> {
    // Get comand line options.
    let cli = Cli::parse();

    if cli.config_path.is_empty() {
        return Err(Error::Cli("Usage: loadtest PATH_TO_CONFIG".to_string()));
    }

    Ok(cli.config_path)
}

pub fn load_config(config_path: String) -> Result<Config, Error> {
    let config = std::fs::read_to_string(config_path)?;
    let config: Config = toml::from_str(&config)?;

    if config.writes == 0 && config.reads > 0 {
        return Err(Error::InvalidConfig(
            "It is invalid to set writes = 0 and reads != 0.".to_string(),
        ));
    }
    if config.max_val_length <= config.min_val_length {
        return Err(Error::InvalidConfig(format!(
            "max_val_length must be greater than min_val_length but {} <= {}",
            config.max_val_length, config.min_val_length,
        )));
    }

    Ok(config)
}

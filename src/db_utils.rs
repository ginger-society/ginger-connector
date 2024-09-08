use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Write;
use std::{fs, str::FromStr};
use toml;

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Config {
    pub branch: String,
    pub organization_id: String,
    pub database: Vec<DatabaseConfig>, // Unified all db types in one vector
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub struct DatabaseConfig {
    pub db_type: DbType, // Use DbType enum
    pub description: String,
    pub enable: bool,
    pub id: Option<String>,
    pub name: String,
    pub port: String,
    pub studio_port: Option<String>,
}

impl fmt::Display for DatabaseConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")] // This will map the enum to/from lowercase strings
pub enum DbType {
    Rdbms,
    DocumentDb,
    Cache,
}

impl fmt::Display for DbType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let db_type_str = match self {
            DbType::Rdbms => "rdbms",
            DbType::DocumentDb => "documentdb",
            DbType::Cache => "cache",
        };
        write!(f, "{}", db_type_str)
    }
}

impl FromStr for DbType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "rdbms" => Ok(DbType::Rdbms),
            "documentdb" => Ok(DbType::DocumentDb),
            "cache" => Ok(DbType::Cache),
            _ => Err(format!("'{}' is not a valid DbType", s)),
        }
    }
}

pub fn read_db_config_v2(file_path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(file_path)?;
    let config: Config = toml::from_str(&contents)?;
    Ok(config)
}

pub fn write_db_config_v2(
    file_path: &str,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let toml_string = toml::to_string(config)?;
    let mut file = fs::File::create(file_path)?;
    file.write_all(toml_string.as_bytes())?;
    Ok(())
}

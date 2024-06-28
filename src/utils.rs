use std::{error::Error, fmt, fs, path::Path, process::exit};

use colored::Colorize;
use inquire::{list_option::ListOption, validator::Validation, MultiSelect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Service {
    pub schema_url: String,
    pub name: String,
}
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum LANG {
    Rust,
    TS,
    Python,
}
// This formatting is for OpenAPI generator
impl fmt::Display for LANG {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LANG::Rust => write!(f, "rust"),
            LANG::TS => write!(f, "typescript-fetch"),
            LANG::Python => write!(f, "Python"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct RepoMetaData {
    version: String,
    services: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct ServiceMetaData {
    envs: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub services: Option<Vec<String>>,
    pub lang: LANG,
    pub dir: String,
    pub repo: String,
}

pub fn read_config_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

fn write_config_file<P: AsRef<Path>>(path: P, config: &Config) -> Result<(), Box<dyn Error>> {
    let content = toml::to_string(config)?;
    fs::write(path, content)?;
    Ok(())
}

#[tokio::main]
pub async fn fetch_metadata_and_process(path: &String, config_path: &Path) {
    let mut config = read_config_file(config_path).unwrap();

    let client = reqwest::Client::new();
    let response = client.get(path).send().await.unwrap();

    if response.status().is_success() {
        let meta_data: RepoMetaData = response.json().await.unwrap();
        println!(
            "Services Repo version : {}",
            format!("{}", &meta_data.version).blue().underline()
        );

        let service_selector_validator = |a: &[ListOption<&String>]| {
            if a.len() < 1 {
                return Ok(Validation::Invalid(
                    "At least one table is required!".into(),
                ));
            }
            Ok(Validation::Valid)
        };

        let mut existing_services_namespace: Vec<usize> = vec![];

        for (itter_count, service_name) in config.services.unwrap().iter().enumerate() {
            if meta_data.services.contains(&service_name) {
                existing_services_namespace.push(itter_count);
            }
        }

        let ans = MultiSelect::new(
            "Select the services you want to add to this project ",
            meta_data.services.clone(),
        )
        .with_validator(service_selector_validator)
        .with_page_size(20)
        .with_default(&existing_services_namespace)
        .prompt();

        config.services = Some(ans.unwrap());
        match write_config_file(config_path, &config){
            Ok(_) => println!("Configuration updated successfully"),
            Err(_) => println!("Counld not save the config file. Please check if you have approperiate permission to write")
        };
    } else {
        println!("Unable to get the metadata for this template");
        exit(1)
    }
}

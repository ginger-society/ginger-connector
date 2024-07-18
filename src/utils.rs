use std::{
    collections::HashMap,
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::{exit, Command},
};

use clap::ValueEnum;
use colored::Colorize;
use inquire::{list_option::ListOption, validator::Validation, MultiSelect};
use serde::{Deserialize, Serialize};
use IAMService::apis::configuration::Configuration as IAMConfiguration;
use MetadataService::apis::{
    configuration::Configuration as MetadataConfiguration,
    default_api::{metadata_get_services_and_envs, MetadataGetServicesAndEnvsParams},
};

#[derive(Debug, Clone)]
pub struct Service {
    pub schema_url: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, ValueEnum)]
pub enum LANG {
    Rust,
    TS,
    Python,
}

impl fmt::Display for LANG {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LANG::Rust => write!(f, "rust"),
            LANG::TS => write!(f, "typescript-fetch"),
            LANG::Python => write!(f, "python"),
        }
    }
}

impl LANG {
    pub fn all() -> Vec<LANG> {
        vec![LANG::Rust, LANG::TS, LANG::Python]
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct Config {
    pub services: Option<HashMap<String, HashMap<String, String>>>,
    pub lang: LANG,
    pub dir: String,
    pub repo: String,
}

pub fn read_config_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn write_config_file<P: AsRef<Path>>(path: P, config: &Config) -> Result<(), Box<dyn Error>> {
    let content = toml::to_string(config)?;
    fs::write(path, content)?;
    Ok(())
}
pub async fn fetch_metadata_and_process(
    config_path: &Path,
    iam_config: &IAMConfiguration,
    metadata_config: &MetadataConfiguration,
) {
    let mut config = read_config_file(config_path).unwrap();

    match metadata_get_services_and_envs(
        metadata_config,
        MetadataGetServicesAndEnvsParams {
            page_number: Some("1".to_string()),
            page_size: Some("50".to_string()),
        },
    )
    .await
    {
        Ok(services) => {
            println!("{:?}", services);

            let service_selector_validator = |a: &[ListOption<&String>]| {
                if a.len() < 1 {
                    return Ok(Validation::Invalid(
                        "At least one service is required!".into(),
                    ));
                }
                Ok(Validation::Valid)
            };

            let mut existing_services_namespace: Vec<usize> = vec![];

            for (itter_count, service_name) in config.services.as_ref().unwrap().iter().enumerate()
            {
                if services.iter().any(|s| s.identifier == *service_name.0) {
                    existing_services_namespace.push(itter_count);
                }
            }

            let service_names: Vec<String> =
                services.iter().map(|s| s.identifier.clone()).collect();

            let ans = MultiSelect::new(
                "Select the services you want to add to this project ",
                service_names.clone(),
            )
            .with_validator(service_selector_validator)
            .with_page_size(20)
            .with_default(&existing_services_namespace)
            .prompt();

            let selected_services = ans.unwrap();
            let mut new_services = HashMap::new();
            for service_name in selected_services.iter() {
                if let Some(service) = services.iter().find(|s| &s.identifier == service_name) {
                    let envs: HashMap<String, String> = service
                        .envs
                        .iter()
                        .map(|env| (env.env_key.clone(), env.base_url.clone()))
                        .collect();
                    new_services.insert(service_name.clone(), envs);
                }
            }

            config.services = Some(new_services);

            match write_config_file(config_path, &config) {
                Ok(_) => println!("Configuration updated successfully"),
                Err(_) => println!("Could not save the config file. Please check if you have appropriate permission to write"),
            };
        }
        Err(_) => {
            println!("Unable to get the metadata for this template");
            exit(1)
        }
    };
}

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

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct RepoMetaData {
    version: String,
    services: HashMap<String, HashMap<String, String>>,
    templates: Vec<TemplateMetaData>,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct TemplateMetaData {
    description: String,
    short_name: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct ServiceMetaData {
    envs: Vec<String>,
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

#[tokio::main]
pub async fn fetch_metadata_and_process(config_path: &Path) {
    let mut config = read_config_file(config_path).unwrap();

    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "https://raw.githubusercontent.com/{}/main/metadata.json",
            config.repo
        ))
        .send()
        .await
        .unwrap();

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
            if meta_data.services.contains_key(service_name.0) {
                existing_services_namespace.push(itter_count);
            }
        }

        let ans = MultiSelect::new(
            "Select the services you want to add to this project ",
            meta_data.services.keys().cloned().collect(),
        )
        .with_validator(service_selector_validator)
        .with_page_size(20)
        .with_default(&existing_services_namespace)
        .prompt();

        let selected_services = ans.unwrap();
        let mut new_services = HashMap::new();
        for service_name in selected_services.iter() {
            if let Some(service_envs) = meta_data.services.get(service_name) {
                new_services.insert(service_name.clone(), service_envs.clone());
            }
        }

        config.services = Some(new_services);

        match write_config_file(config_path, &config) {
            Ok(_) => println!("Configuration updated successfully"),
            Err(_) => println!("Could not save the config file. Please check if you have appropriate permission to write"),
        };
    } else {
        println!("Unable to get the metadata for this template");
        exit(1)
    }
}

fn open_api_client_generator(service: &Service, lang: LANG, root_dir: &str, env: &str) {
    let output_dir = format!("{}/{}_client", root_dir, service.name);
    println!("Generating client for: {}", service.name);

    let output = Command::new("openapi-generator")
        .arg("generate")
        .arg("-g")
        .arg(lang.to_string())
        .arg("-o")
        .arg(&output_dir)
        .arg("--additional-properties")
        .arg(format!(
            "useSingleRequestParameter=true,packageName={}",
            service.name
        ))
        .arg("-i")
        .arg(service.schema_url.clone())
        .output();

    match output {
        Ok(cmd_output) => {
            for line in String::from_utf8(cmd_output.stdout)
                .unwrap()
                .split('\n')
                .into_iter()
            {
                println!("{}", line)
            }

            match lang {
                LANG::Rust => todo!(),
                LANG::TS => {
                    // Add content to index.ts
                    let index_ts_path = format!("{}/index.ts", output_dir);
                    let index_ts_content = format!(
                        r#"/* tslint:disable */
/* eslint-disable */

import {{ DefaultApi }} from './apis'
import {{ Configuration }} from './runtime'

export * from './runtime';
export * from './apis/index';
export * from './models/index';

const configuration = new Configuration({{
    basePath: '{}'
}})
const client = new DefaultApi(configuration)
export default client
"#,
                        env
                    );

                    match OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&index_ts_path)
                    {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(index_ts_content.as_bytes()) {
                                eprintln!("Error writing to index.ts: {:?}", e);
                            }
                        }
                        Err(e) => eprintln!("Error creating index.ts: {:?}", e),
                    }
                }
                LANG::Python => todo!(),
            }
        }
        Err(err) => {
            eprintln!("{:?}", err)
        }
    }
}

fn print_openapi_generator_not_found() {
    println!(
        "The OpenAPI generator is not installed on your machine. Please use {} on MacOS / Windows / Linux",
        "npm install @openapitools/openapi-generator-cli -g".green()
    );
    exit(1);
}

pub fn generate_client(config_path: &Path, env: &str) {
    match Command::new("java").arg("-version").output() {
        Ok(cmd_output) => {
            if !String::from_utf8(cmd_output.stderr)
                .unwrap()
                .contains("java version")
            {
                println!("Java is not installed on your machine. Please use https://www.java.com/en/download/help/download_options.html to install Java first");
                exit(1);
            } else {
                match Command::new("openapi-generator").arg("--version").output() {
                    Ok(cmd_output) => {
                        if !String::from_utf8(cmd_output.stdout)
                            .unwrap()
                            .contains("openapi-generator-cli")
                        {
                            print_openapi_generator_not_found();
                        }
                    }
                    Err(_) => {
                        print_openapi_generator_not_found();
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("{:?}", err)
        }
    }

    let services_config = match read_config_file(config_path) {
        Ok(c) => c,
        Err(_) => {
            println!(
                "There is no service configuration found. Please use {} to add one. Exiting",
                "ginger-connector init".blue()
            );
            exit(1);
        }
    };

    println!("{:?}", services_config);

    for (service_name, service_envs) in services_config.services.unwrap().iter() {
        let env_url = service_envs.get(env).unwrap();
        open_api_client_generator(
            &Service {
                schema_url: format!(
                    "https://raw.githubusercontent.com/{}/main/{}/{}.json",
                    services_config.repo, service_name, env
                ),
                name: service_name.to_string(),
            },
            services_config.lang,
            &services_config.dir,
            env_url,
        );
    }
}

use crate::{
    utils::{read_config_file, Service, LANG},
    Environment,
};
use colored::Colorize;
use reqwest::Client;
use std::{fs, path::Path, process::exit};
use MetadataService::{
    apis::{
        configuration::Configuration as MetadataConfiguration,
        default_api::{
            metadata_get_services_and_envs, metadata_update_or_create_service,
            MetadataGetServicesAndEnvsParams, MetadataUpdateOrCreateServiceParams,
        },
    },
    models::UpdateServiceRequest,
};

pub async fn publish_metadata(
    config_path: &Path,
    env: Environment,
    metadata_config: &MetadataConfiguration,
) {
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

    let cargo_toml_content = fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");

    let cargo_toml: toml::Value =
        toml::from_str(&cargo_toml_content).expect("Failed to parse Cargo.toml");

    if let Some(package) = cargo_toml.get("package") {
        if let Some(name) = package.get("name") {
            println!("Package name: {}", name.as_str().unwrap());
        } else {
            eprintln!("'name' field not found in Cargo.toml");
        }
        if let Some(version) = package.get("version") {
            println!("Package version: {}", version.as_str().unwrap());
        } else {
            eprintln!("'version' field not found in Cargo.toml");
        }

        // Make a GET request to {env}/{services_config.spec_url}
        let client = Client::new();
        let env_base_url = match env {
            Environment::Dev => &services_config.urls["dev"],
            Environment::Stage => &services_config.urls["stage"],
            Environment::Prod => &services_config.urls["prod"],
        };
        let full_url = format!("{}/{}", env_base_url, services_config.spec_url);

        match client.get(&full_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match metadata_update_or_create_service(
                        metadata_config,
                        MetadataUpdateOrCreateServiceParams {
                            update_service_request: UpdateServiceRequest {
                                identifier: package
                                    .get("name")
                                    .unwrap()
                                    .as_str()
                                    .unwrap()
                                    .to_string(),
                                env: env.to_string(),
                                base_url: env_base_url.clone(),
                                spec: response.text().await.unwrap(),
                            },
                        },
                    )
                    .await
                    {
                        Ok(response) => {
                            println!("{:?}", response)
                        }
                        Err(_) => {}
                    };
                } else {
                    eprintln!("Failed to fetch the spec: {}", response.status());
                }
            }
            Err(e) => {
                eprintln!("Error making the GET request: {:?}", e);
            }
        }
    } else {
        eprintln!("'package' section not found in Cargo.toml");
    }
}

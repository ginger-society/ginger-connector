use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use colored::Colorize;
use ginger_shared_rs::{read_service_config_file, Service};
use MetadataService::apis::configuration::Configuration as MetadataConfiguration;
use MetadataService::apis::default_api::{metadata_get_service_and_env_by_id, MetadataGetServiceAndEnvByIdParams};
use ginger_shared_rs::Environment;
use crate::service::{extract_org_and_package, open_api_client_generator};


pub async fn handle_ws_event(service_name: &str, metadata_config: &MetadataConfiguration, config_path: &Path) {
    let services_config = match read_service_config_file(config_path) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Error reading config: {:?}", err);
            eprintln!(
                "There is no service configuration found. Please use {} to add one.",
                "ginger-connector init".blue()
            );
            return;
        }
    };

    let ginger_tmp_dir = PathBuf::from(".ginger.tmp");
    if !ginger_tmp_dir.exists() {
        if let Err(e) = fs::create_dir(&ginger_tmp_dir) {
            eprintln!("Error creating .ginger.tmp directory: {:?}", e);
            return;
        }
    }

    let environment = Environment::Prod;

    let services = match &services_config.services {
        Some(s) => s,
        None => {
            eprintln!("'services' field missing in service config file");
            return;
        }
    };

    let service_urls = match services.get(service_name) {
        Some(urls) => urls,
        None => {
            eprintln!(
                "{}: Service '{}' not found in service configuration.",
                "Warning".yellow(),
                service_name
            );
            return;
        }
    };

    let base_url = match environment {
        Environment::Dev => service_urls.get("dev"),
        Environment::Stage => service_urls.get("stage"),
        Environment::Prod => service_urls.get("prod"),
        Environment::ProdK8 => service_urls.get("prod_k8"),
        Environment::StageK8 => service_urls.get("stage_k8"),
    };

    let base_url = match base_url {
        Some(url) => url,
        None => {
            eprintln!(
                "URL not found for environment {:?} in service '{:?}'",
                "Warning".yellow(),
                service_name
            );
            return;
        }
    };

    if let Some((org_id, package_name)) = extract_org_and_package(service_name) {
        println!("org_id: {}, package_name: {}", org_id, package_name);

        match metadata_get_service_and_env_by_id(
            metadata_config,
            MetadataGetServiceAndEnvByIdParams {
                service_identifier: package_name.clone(),
                env: environment.to_string(),
                org_id: org_id.clone(),
            },
        )
        .await
        {
            Ok(response) => {
                let spec_path = ginger_tmp_dir.join(format!(
                    "{}@{}.{}.spec.json",
                    package_name, org_id, environment
                ));

                match OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&spec_path)
                {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(response.spec.as_bytes()) {
                            eprintln!("Error writing to {}: {:?}", spec_path.display(), e);
                        }
                    }
                    Err(e) => eprintln!("Error creating {}: {:?}", spec_path.display(), e),
                }

                open_api_client_generator(
                    &Service {
                        schema_url: spec_path.to_string_lossy().to_string(),
                        name: package_name.to_string(),
                    },
                    services_config.lang,
                    &services_config.dir.clone().unwrap_or_else(|| ".".into()),
                    base_url,
                );
            }
            Err(e) => {
                eprintln!("Failed to fetch metadata for {}: {:?}", package_name, e);
            }
        }
    } else {
        eprintln!("Input '{}' is not in the expected format", service_name);
    }
}

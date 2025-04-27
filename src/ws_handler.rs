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
    // load your services config
    let services_config = match read_service_config_file(config_path) {
        Ok(c) => c,
        Err(err) => {
            println!("{:?}", err);
            println!(
                "There is no service configuration found. Please use {} to add one. Exiting",
                "ginger-connector init".blue()
            );
            exit(1);
        }
    };

    println!("{:?}", services_config);

    // Ensure .ginger.tmp directory exists
    let ginger_tmp_dir = PathBuf::from(".ginger.tmp");
    if !ginger_tmp_dir.exists() {
        if let Err(e) = fs::create_dir(&ginger_tmp_dir) {
            eprintln!("Error creating .ginger.tmp directory: {:?}", e);
            exit(1);
        }
    }
    let environment = Environment::Prod;

    let services = services_config.services
    .as_ref()
    .expect("services field is missing in service config");

    let service_urls = services.get(service_name)
    .expect(&format!("Service '{}' not found in service configuration", service_name));

    let base_url = match environment {
        Environment::Dev => service_urls["dev"].clone(),
        Environment::Stage => service_urls["stage"].clone(),
        Environment::Prod => service_urls["prod"].clone(),
        Environment::ProdK8 => service_urls["prod_k8"].clone(),
        Environment::StageK8 => service_urls["stage_k8"].clone(),
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
                    package_name.clone(),
                    org_id.clone(),
                    environment
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
            }
            Err(e) => {
                println!("{:?}", e)
            }
        }

        open_api_client_generator(
            &Service {
                schema_url: format!(
                    ".ginger.tmp/{}@{}.{}.spec.json",
                    package_name, org_id, environment
                ),
                name: package_name.to_string(),
            },
            services_config.lang,
            &services_config.dir.clone().unwrap(),
            &base_url,
        );
    } else {
        println!("Input is not in the expected format");
    }
}

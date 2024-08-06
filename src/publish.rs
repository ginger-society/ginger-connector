use crate::{
    utils::{read_config_file, read_db_config, Service, LANG},
    Environment,
};
use colored::Colorize;
use reqwest::Client;
use serde_json::Value as JsonValue;
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    process::exit,
    time::Duration,
};
use toml::Value;
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

pub fn get_package_json_info() -> Option<(String, String, String, String)> {
    let mut file = File::open("package.json").expect("Failed to open package.json");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Failed to read package.json");

    let package_json: JsonValue =
        serde_json::from_str(&content).expect("Failed to parse package.json");

    let name = package_json.get("name")?.as_str()?.to_string();
    let version = package_json.get("version")?.as_str()?.to_string();
    let description = package_json.get("description")?.as_str()?.to_string();

    // Extract organization and package name
    let (organization, package_name) = if name.starts_with('@') {
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() == 2 {
            (
                parts[0].trim_start_matches('@').to_string(),
                parts[1].to_string(),
            )
        } else {
            println!("The package name should be of format @scope/pkg-name");
            exit(1);
        }
    } else {
        println!("The package name should be of format @scope/pkg-name");
        exit(1);
    };
    Some((package_name, version, description, organization))
}

pub fn get_cargo_toml_info() -> Option<(String, String, String, String)> {
    let cargo_toml_content = fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");
    let cargo_toml: Value =
        toml::from_str(&cargo_toml_content).expect("Failed to parse Cargo.toml");

    if let Some(package) = cargo_toml.get("package") {
        let name = package.get("name")?.as_str()?.to_string();
        let version = package.get("version")?.as_str()?.to_string();
        let description = package.get("description")?.as_str()?.to_string();

        let metadata = cargo_toml
            .get("package")
            .and_then(|pkg| pkg.get("metadata"))
            .expect("there is no metadata field in your cargo.toml");
        let organization = metadata.get("organization")?.as_str()?.to_string();

        Some((name, version, description, organization))
    } else {
        None
    }
}

pub fn get_pyproject_toml_info() -> Option<(String, String, String, String)> {
    let pyproject_toml_content =
        fs::read_to_string("pyproject.toml").expect("Failed to read pyproject.toml");
    let pyproject_toml: Value =
        toml::from_str(&pyproject_toml_content).expect("Failed to parse pyproject.toml");

    if let Some(project) = pyproject_toml.get("project") {
        let name = project.get("name")?.as_str()?.to_string();
        let version = project.get("version")?.as_str()?.to_string();
        let description = project.get("description")?.as_str()?.to_string();
        let organization = project.get("organization")?.as_str()?.to_string();

        Some((name, version, description, organization))
    } else {
        None
    }
}

async fn fetch_swagger_spec(
    client: &Client,
    url: &str,
    expected_version: &str,
    retry_interval: Duration,
    max_retries: usize,
) -> Option<String> {
    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(retry_interval).await;
        }

        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    let spec_text = response.text().await.unwrap();
                    let spec_json: JsonValue = serde_json::from_str(&spec_text).unwrap();
                    let swagger_version = spec_json.get("info")?.get("version")?.as_str()?;
                    if swagger_version == expected_version {
                        return Some(spec_text);
                    } else {
                        eprintln!(
                            "Version mismatch: expected {}, got {}",
                            expected_version, swagger_version
                        );
                    }
                } else {
                    eprintln!("Failed to fetch the spec: {}", response.status());
                }
            }
            Err(e) => {
                eprintln!("Error making the GET request: {:?}", e);
            }
        }
    }
    None
}

pub async fn publish_metadata(
    config_path: &Path,
    env: Environment,
    metadata_config: &MetadataConfiguration,
) {
    let services_config = match read_config_file(config_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{:?}", e);
            println!(
                "There is no service configuration found or the existing one is invalid. Please use {} to add one. Exiting",
                "ginger-connector init".blue()
            );
            exit(1);
        }
    };
    println!("{:?}", services_config);

    let (mut name, version, description, organization) = match services_config.lang {
        LANG::TS => get_package_json_info().unwrap_or_else(|| {
            eprintln!("Failed to get name and version from package.json");
            exit(1);
        }),
        LANG::Rust => get_cargo_toml_info().unwrap_or_else(|| {
            eprintln!("Failed to get name and version from Cargo.toml");
            exit(1);
        }),
        LANG::Python => get_pyproject_toml_info().unwrap_or_else(|| {
            eprintln!("Failed to get name and version from pyproject.toml");
            exit(1);
        }),
    };

    if services_config.override_name.is_some() {
        name = services_config.override_name.unwrap()
    }

    println!("Package name: {}", name);
    println!("Package version: {}", version);
    println!("Package organization: {}", organization);
    println!("Package description: {}", description);

    let client = Client::new();
    let env_base_url_swagger = match env {
        Environment::Dev => &services_config.urls.clone().unwrap()["dev"],
        Environment::Stage => &services_config.urls.clone().unwrap()["stage"],
        Environment::Prod => &services_config.urls.clone().unwrap()["prod"],
        Environment::ProdK8 => &services_config.urls.clone().unwrap()["prod"],
        Environment::StageK8 => &services_config.urls.clone().unwrap()["stage"],
    };

    let env_base_url = match env {
        Environment::Dev => &services_config.urls.clone().unwrap()["dev"],
        Environment::Stage => &services_config.urls.clone().unwrap()["stage"],
        Environment::Prod => &services_config.urls.clone().unwrap()["prod"],
        Environment::ProdK8 => &services_config.urls.clone().unwrap()["prod_k8"],
        Environment::StageK8 => &services_config.urls.clone().unwrap()["stage_k8"],
    };

    let spec_url = services_config.spec_url.clone();
    let spec = if let Some(spec_url) = spec_url {
        let full_url = format!("{}{}", env_base_url_swagger, spec_url);

        if env != Environment::Dev {
            tokio::time::sleep(Duration::from_secs(10)).await;
        }

        fetch_swagger_spec(&client, &full_url, &version, Duration::from_secs(10), 3).await.unwrap_or_else(|| {
            eprintln!("Failed to fetch the spec with the expected version after multiple attempts. Aborting metadata publishing.");
            exit(1);
        })
    } else {
        String::new()
    };

    let db_config_path = Path::new("database.toml");
    let (tables, schema_id) = match read_db_config(db_config_path) {
        Ok(config) => (config.tables.names, Some(config.schema.schema_id)),
        Err(_) => (vec![], None),
    };

    let dependencies_list = services_config.services.unwrap().keys().cloned().collect();

    match metadata_update_or_create_service(
        metadata_config,
        MetadataUpdateOrCreateServiceParams {
            update_service_request: UpdateServiceRequest {
                identifier: name,
                env: env.to_string(),
                base_url: env_base_url.clone(),
                spec,
                dependencies: dependencies_list,
                tables,
                db_schema_id: schema_id,
                service_type: Some(services_config.service_type),
                version: Some(Some(version)),
                lang: Some(
                    services_config
                        .lang
                        .to_string()
                        .split('-')
                        .next()
                        .map(|part| part.to_string()),
                ),
                description: description,
                organization_id: organization,
            },
        },
    )
    .await
    {
        Ok(response) => {
            println!("{:?}", response)
        }
        Err(e) => {
            println!("{:?}", e)
        }
    };
}
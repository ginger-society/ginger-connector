use crate::Environment;
use colored::Colorize;
use ginger_shared_rs::{
    read_consumer_db_config, read_package_metadata_file, read_releaser_config_file,
    read_service_config_file, LANG,
};
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
        default_api::{metadata_update_or_create_service, MetadataUpdateOrCreateServiceParams},
    },
    models::UpdateServiceRequest,
};

pub fn get_package_json_info() -> Option<(String, String, String, String, Vec<String>)> {
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

    // Internal dependencies logic
    let prefix = format!("@{}/", organization);
    let mut internal_dependencies = Vec::new();

    if let Some(dependencies) = package_json.get("dependencies").and_then(|d| d.as_object()) {
        for (key, _) in dependencies {
            if key.starts_with(&prefix) {
                internal_dependencies.push(key.clone());
            }
        }
    }

    if let Some(dev_dependencies) = package_json
        .get("devDependencies")
        .and_then(|d| d.as_object())
    {
        for (key, _) in dev_dependencies {
            if key.starts_with(&prefix) {
                internal_dependencies.push(key.clone());
            }
        }
    }

    Some((
        package_name,
        version,
        description,
        organization,
        internal_dependencies,
    ))
}

pub fn get_cargo_toml_info() -> Option<(String, String, String, String, Vec<String>)> {
    let cargo_toml_content = fs::read_to_string("Cargo.toml").expect("Failed to read Cargo.toml");
    let cargo_toml: Value =
        toml::from_str(&cargo_toml_content).expect("Failed to parse Cargo.toml");

    if let Some(package) = cargo_toml.get("package") {
        let name = package.get("name")?.as_str()?.to_string();
        let version = package.get("version")?.as_str()?.to_string();
        let description = package.get("description")?.as_str()?.to_string();
        let mut internal_dependencies = Vec::new();

        let metadata = cargo_toml
            .get("package")
            .and_then(|pkg| pkg.get("metadata"))
            .expect("there is no metadata field in your cargo.toml");
        let organization = metadata.get("organization")?.as_str()?.to_string();

        // Extract dependencies
        let dependencies = cargo_toml
            .get("dependencies")
            .expect("there is no dependencies field in your Cargo.toml");

        if let Some(deps) = dependencies.as_table() {
            for (key, value) in deps {
                if let Some(dep_table) = value.as_table() {
                    // Check if the dependency has an organization field
                    if let Some(dep_org) = dep_table.get("organization") {
                        if dep_org.as_str()? == organization {
                            let dep_format = format!("@{}/{}", organization, key);
                            internal_dependencies.push(dep_format);
                        }
                    }
                }
            }
        }

        Some((
            name,
            version,
            description,
            organization,
            internal_dependencies,
        ))
    } else {
        None
    }
}

pub fn get_pyproject_toml_info() -> Option<(String, String, String, String, Vec<String>)> {
    // Read and parse pyproject.toml
    let pyproject_toml_content =
        fs::read_to_string("pyproject.toml").expect("Failed to read pyproject.toml");
    let pyproject_toml: Value =
        toml::from_str(&pyproject_toml_content).expect("Failed to parse pyproject.toml");

    let name = pyproject_toml.get("name")?.as_str()?.to_string();
    let version = pyproject_toml.get("version")?.as_str()?.to_string();
    let description = pyproject_toml.get("description")?.as_str()?.to_string();
    let organization = pyproject_toml.get("organization")?.as_str()?.to_string();

    // Read and process requirements.txt
    let requirements_path = Path::new("requirements.txt");
    let mut dependencies = Vec::new();

    if requirements_path.exists() {
        let requirements_content =
            fs::read_to_string(requirements_path).expect("Failed to read requirements.txt");

        for line in requirements_content.lines() {
            let trimmed_line = line.trim();

            if trimmed_line.is_empty() {
                continue; // Skip empty lines
            }

            // If the line starts with '#', treat it as an internal dependency
            if trimmed_line.starts_with('#') {
                let internal_dependency = trimmed_line.trim_start_matches('#').trim();
                dependencies.push(internal_dependency.to_string());
                continue;
            }

            // Split the line on '#', if any
            let parts: Vec<&str> = trimmed_line.split('#').collect();
            let mut dependency = parts[0].trim().to_string();

            // Remove ==version if present
            if let Some((dep_name, _version)) = dependency.split_once("==") {
                dependency = dep_name.to_string();
            }

            if parts.len() > 1 {
                let org = parts[1].trim();

                // Check if the organization matches the one from pyproject.toml
                if org == organization {
                    dependencies.push(format!("@{}/{}", org, dependency));
                }
            }
        }
    }

    Some((name, version, description, organization, dependencies))
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
    releaser_path: &Path,
    package_path: &Path,
) {
    let package_metadata = read_package_metadata_file(package_path).unwrap();

    let links_str = serde_json::to_string(&package_metadata.links).unwrap();

    let services_config = match read_service_config_file(config_path) {
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

    let releaser_config = match read_releaser_config_file(releaser_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
    };

    let (mut name, version, description, organization, dependencies) = match services_config.lang {
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
        LANG::Shell => todo!(),
    };

    if services_config.override_name.is_some() {
        name = services_config.override_name.unwrap()
    }

    println!("Package name: {}", name);
    println!("Package version: {}", version);
    println!("Package organization: {}", organization);
    println!("Package description: {}", description);
    println!("git: {:?}", releaser_config.settings.git_url_prefix);

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

    let env_base_url_ws = match env {
        Environment::Dev => &services_config.urls_ws.clone().unwrap()["dev"],
        Environment::Stage => &services_config.urls_ws.clone().unwrap()["stage"],
        Environment::Prod => &services_config.urls_ws.clone().unwrap()["prod"],
        Environment::ProdK8 => &services_config.urls_ws.clone().unwrap()["prod_k8"],
        Environment::StageK8 => &services_config.urls_ws.clone().unwrap()["stage_k8"],
    };
    println!("env_base_url_ws: {:?} , ", env_base_url_ws);
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
    let (tables, schema_id, cache_schema_id, message_queue_schema_id) =
        match read_consumer_db_config(db_config_path) {
            Ok(config) => (
                config.tables.names,
                Some(config.schema.schema_id),
                Some(config.schema.cache_schema_id),
                Some(config.schema.message_queue_schema_id),
            ),
            Err(_) => (vec![], None, None, None),
        };

    let mut dependencies_list: Vec<String> =
        services_config.services.unwrap().keys().cloned().collect();
    dependencies_list.extend(dependencies);
    println!("{:?}", dependencies_list);
    match metadata_update_or_create_service(
        metadata_config,
        MetadataUpdateOrCreateServiceParams {
            update_service_request: UpdateServiceRequest {
                identifier: name,
                env: env.to_string(),
                base_url: env_base_url.clone(),
                base_url_ws: Some(Some(env_base_url_ws.clone())),
                spec,
                dependencies: dependencies_list,
                tables,
                db_schema_id: schema_id,
                cache_schema_id: cache_schema_id,
                message_queue_schema_id: message_queue_schema_id,
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
                repo_origin: Some(releaser_config.settings.git_url_prefix),
                quick_links: Some(Some(links_str)),
            },
        },
    )
    .await
    {
        Ok(response) => {
            println!("{:?}", response)
        }
        Err(e) => {
            println!("Error updating / creating the service {:?}", e)
        }
    };
}

use std::{collections::HashMap, fs::File, io::BufReader, path::Path, process::exit};

use colored::Colorize;
use ginger_shared_rs::{
    read_package_metadata_file, read_releaser_config_file, read_service_config_file,
    write_service_config_file, LANG,
};
use inquire::{list_option::ListOption, validator::Validation, MultiSelect};
use serde_json::Value;
use IAMService::apis::configuration::Configuration as IAMConfiguration;
use MetadataService::{
    apis::{
        configuration::Configuration as MetadataConfiguration,
        default_api::{
            metadata_create_dbschema, metadata_create_or_update_package,
            metadata_get_services_and_envs, metadata_update_dbschema,
            metadata_update_pipeline_status, MetadataCreateDbschemaParams,
            MetadataCreateOrUpdatePackageParams, MetadataGetServicesAndEnvsParams,
            MetadataUpdateDbschemaParams, MetadataUpdatePipelineStatusParams,
        },
    },
    models::{
        CreateDbschemaRequest, CreateOrUpdatePackageRequest, PipelineStatusUpdateRequest,
        UpdateDbschemaRequest,
    },
};

use crate::{
    db_utils::{read_db_config_v2, write_db_config_v2},
    publish::{get_cargo_toml_info, get_package_json_info, get_pyproject_toml_info},
    Environment,
};

pub async fn update_pipeline(
    package_path: &Path,
    metadata_config: &MetadataConfiguration,
    config_path: &Path,
    env: Environment,
    status: String,
) {
    let metadata_details = read_package_metadata_file(package_path).unwrap();

    let (mut name, version, description, organization, internal_dependencies) =
        match metadata_details.lang {
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

    let mut update_type = "package".to_string();

    if services_config.service_type.is_some() {
        update_type = "service".to_string();
    }

    println!(
        "{:?} , {:?}, {:?}, {:?} , {:?}",
        name,
        organization,
        update_type,
        env.to_string(),
        status
    );

    match metadata_update_pipeline_status(
        &metadata_config,
        MetadataUpdatePipelineStatusParams {
            pipeline_status_update_request: {
                PipelineStatusUpdateRequest {
                    env: env.to_string(),
                    status,
                    update_type,
                    org_id: organization,
                    identifier: name,
                }
            },
        },
    )
    .await
    {
        Ok(status) => {
            println!("{:?}", status);
        }
        Err(e) => {
            println!("{:?}", e);
        }
    }
}
pub async fn register_db(metadata_config: &MetadataConfiguration, releaser_path: &Path) {
    let mut db_config = match read_db_config_v2("db-compose.toml") {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error reading db-compose.toml: {:?}", err);
            return;
        }
    };

    let releaser_config = match read_releaser_config_file(releaser_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
    };

    // Iterate over all the databases in the configuration
    for db in &mut db_config.database {
        match &db.id {
            Some(id) if !id.is_empty() => {
                // If db.id exists and is not an empty string
                println!("Database '{}' has ID: {}", db.name, id);

                match metadata_update_dbschema(
                    &metadata_config,
                    MetadataUpdateDbschemaParams {
                        update_dbschema_request: UpdateDbschemaRequest {
                            name: db.name.clone(),
                            description: Some(Some(db.description.clone())),
                            organisation_id: db_config.organization_id.clone(),
                            repo_origin: releaser_config.clone().settings.git_url_prefix.unwrap(),
                            version: releaser_config.version.formatted(),
                        },
                        schema_id: id.clone(),
                        branch_name: db_config.branch.clone(),
                    },
                )
                .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        println!("{:?}", e);
                    }
                };
            }
            _ => {
                // Handle case when db.id is None or an empty string
                println!("Database '{}' is missing a valid ID", db.name);
                match metadata_create_dbschema(
                    &metadata_config,
                    MetadataCreateDbschemaParams {
                        create_dbschema_request: CreateDbschemaRequest {
                            name: db.name.clone(),
                            description: Some(Some(db.description.clone())),
                            data: None,
                            organisation_id: db_config.organization_id.clone(),
                            db_type: db.db_type.to_string(),
                            repo_origin: releaser_config.clone().settings.git_url_prefix.unwrap(),
                            version: releaser_config.version.formatted(),
                        },
                    },
                )
                .await
                {
                    Ok(resp) => {
                        // Update db.id with the newly created schema identifier
                        db.id = Some(resp.identifier.clone());
                    }
                    Err(err) => {
                        print!("{:?}", err)
                    }
                }
            }
        }
    }

    write_db_config_v2("db-compose.toml", &db_config).unwrap();
}

pub async fn register_package(
    package_path: &Path,
    iam_config: &IAMConfiguration,
    metadata_config: &MetadataConfiguration,
    config_path: &Path,
    env: Environment,
    releaser_path: &Path,
) {
    let metadata_details = read_package_metadata_file(package_path).unwrap();
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

    let releaser_config = match read_releaser_config_file(releaser_path) {
        Ok(c) => c,
        Err(e) => {
            println!("{:?}", e);
            exit(1);
        }
    };

    let mut dependencies_list: Vec<String> =
        services_config.services.unwrap().keys().cloned().collect();

    let (mut name, version, description, organization, internal_dependencies) =
        match metadata_details.lang {
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

    println!("{:?} {:?}", releaser_config, version);

    dependencies_list.extend(internal_dependencies);

    match metadata_create_or_update_package(
        metadata_config,
        MetadataCreateOrUpdatePackageParams {
            create_or_update_package_request: CreateOrUpdatePackageRequest {
                identifier: name,
                package_type: metadata_details.package_type,
                lang: metadata_details.lang.to_string(),
                version,
                organization_id: organization,
                description,
                dependencies: dependencies_list,
                env: env.to_string(),
                repo_origin: Some(releaser_config.settings.git_url_prefix),
            },
        },
    )
    .await
    {
        Ok(response) => {
            println!("{:?}", response);
        }
        Err(e) => {
            println!("{:?}", e);
            println!("Unable to register this package")
        }
    };
}

pub async fn fetch_metadata_and_process(
    config_path: &Path,
    iam_config: &IAMConfiguration,
    metadata_config: &MetadataConfiguration,
) {
    let mut config = read_service_config_file(config_path).unwrap();
    println!("{:?}", config);
    match metadata_get_services_and_envs(
        metadata_config,
        MetadataGetServicesAndEnvsParams {
            page_number: Some("1".to_string()),
            page_size: Some("50".to_string()),
            org_id: config.organization_id.clone(),
        },
    )
    .await
    {
        Ok(services) => {
            println!("{:?}", services);

            let (
                mut current_package_name,
                version,
                description,
                organization,
                internal_dependencies,
            ) = match config.lang {
                LANG::TS => get_package_json_info().unwrap_or_else(|| {
                    eprintln!("Failed to get name and version from package.json");
                    exit(1);
                }),
                LANG::Rust => get_cargo_toml_info().unwrap_or_else(|| {
                    eprintln!("Failed to get name and version from Cargo.toml");
                    exit(1);
                }),
                LANG::Python => {
                    // Implement similar logic for Python if needed
                    unimplemented!()
                }
            };

            if config.override_name.is_some() {
                current_package_name = config.override_name.clone().unwrap()
            }

            println!("Package name: {}", current_package_name);
            println!("Package version: {}", version);
            println!("Package organization: {}", organization);
            println!("Package description: {}", description);

            let service_selector_validator = |a: &[ListOption<&String>]| {
                if a.len() < 1 {
                    return Ok(Validation::Invalid(
                        "At least one service is required!".into(),
                    ));
                }
                Ok(Validation::Valid)
            };

            let mut existing_services_namespace: Vec<usize> = vec![];

            let service_names: Vec<String> = services
                .iter()
                .filter(|s| s.identifier != current_package_name)
                .map(|s| s.identifier.clone())
                .collect();

            // Collect default selections for both services and portals
            for (index, service_name) in service_names.iter().enumerate() {
                if config
                    .services
                    .as_ref()
                    .map_or(false, |s| s.contains_key(service_name))
                {
                    existing_services_namespace.push(index);
                } else if config
                    .portals_refs
                    .as_ref()
                    .map_or(false, |p| p.contains_key(service_name))
                {
                    existing_services_namespace.push(index);
                }
            }

            let service_names: Vec<String> = services
                .iter()
                .filter(|s| s.identifier != current_package_name)
                .map(|s| format!("@{}/{}", s.organization_id.clone(), s.identifier.clone()))
                .collect();

            let ans = MultiSelect::new(
                "Select the services you want to add to this project ",
                service_names.clone(),
            )
            .with_validator(service_selector_validator)
            .with_page_size(20)
            .with_default(&existing_services_namespace)
            .prompt();

            let selected_services = ans.unwrap();
            println!("{:?}", selected_services);
            let mut new_services = HashMap::new();

            let mut new_portal_refs = HashMap::new();

            for service_name in selected_services.iter() {
                if let Some(service) = services
                    .iter()
                    .find(|s| format!("@{}/{}", &s.organization_id, &s.identifier) == *service_name)
                {
                    let envs: HashMap<String, String> = service
                        .envs
                        .iter()
                        .map(|env| (env.env_key.clone(), env.base_url.clone()))
                        .collect();

                    match service.service_type.clone().unwrap().unwrap().as_str() {
                        "Portal" => {
                            new_portal_refs.insert(service_name.clone(), envs);
                        }
                        "RPCEndpoint" => {
                            new_services.insert(service_name.clone(), envs);
                        }
                        _ => {
                            println!(
                                "Unknown service type for {}: {}",
                                service_name,
                                service.service_type.clone().unwrap().unwrap()
                            );
                        }
                    }
                }
            }
            println!("{:?}", new_services);
            config.services = Some(new_services);
            config.portals_refs = Some(new_portal_refs);

            match write_service_config_file(config_path, &config) {
                Ok(_) => println!("Configuration updated successfully"),
                Err(_) => println!("Could not save the config file. Please check if you have appropriate permission to write"),
            };
        }
        Err(e) => {
            println!("{:?}", e);
            println!("Unable to get the metadata for this template");
            exit(1)
        }
    };
}

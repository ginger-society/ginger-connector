use reqwest::Client;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    process::{exit, Command},
    time::Duration,
};
use tokio::time::sleep;

use colored::Colorize;
use ginger_shared_rs::{
    read_db_config, read_package_metadata_file, read_releaser_config_file,
    read_service_config_file, write_db_config, write_service_config_file, LANG,
};
use inquire::{list_option::ListOption, validator::Validation, MultiSelect};
use serde_json::json;
use IAMService::apis::configuration::Configuration as IAMConfiguration;
use MetadataService::{
    apis::{
        configuration::Configuration as MetadataConfiguration,
        default_api::{
            metadata_create_dbschema, metadata_create_or_update_package,
            metadata_get_dbschemas_and_tables, metadata_get_package_version,
            metadata_get_package_version_plain_text, metadata_get_services_and_envs,
            metadata_get_user_packages, metadata_update_dbschema, metadata_update_pipeline_status,
            MetadataCreateDbschemaParams, MetadataCreateOrUpdatePackageParams,
            MetadataGetDbschemasAndTablesParams, MetadataGetPackageVersionParams,
            MetadataGetPackageVersionPlainTextParams, MetadataGetServicesAndEnvsParams,
            MetadataGetUserPackagesParams, MetadataUpdateDbschemaParams,
            MetadataUpdatePipelineStatusParams,
        },
    },
    models::{
        CreateDbschemaRequest, CreateOrUpdatePackageRequest, PipelineStatusUpdateRequest,
        UpdateDbschemaRequest,
    },
};

use crate::{
    publish::{get_cargo_toml_info, get_package_json_info, get_pyproject_toml_info},
    refresher::update_python_internal_dependency,
    Environment,
};

fn extract_org_and_package(input: &str) -> Option<(String, String)> {
    // Ensure the input starts with '@' and contains '/'
    if input.starts_with('@') && input.contains('/') {
        // Split the input at '/' and collect the parts
        let parts: Vec<&str> = input.split('/').collect();

        // Ensure we have exactly 2 parts (org and package_name)
        if parts.len() == 2 {
            let org = parts[0].trim_start_matches('@').to_string(); // Remove the '@' from org
            let package_name = parts[1].to_string(); // Package name
            return Some((org, package_name));
        }
    }

    None // Return None if the input is not in the expected format
}

pub async fn update_pipeline(
    package_path: &Path,
    metadata_config: &MetadataConfiguration,
    config_path: &Path,
    env: Environment,
    status: String,
) {
    let metadata_details = read_package_metadata_file(package_path).unwrap();

    let links_str = serde_json::to_string(&metadata_details.links).unwrap();

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
            LANG::Shell => todo!(),
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
            println!("Error calling metadata_update_pipeline_status{:?}", e);
        }
    }
}
pub async fn register_db(metadata_config: &MetadataConfiguration, releaser_path: &Path) {
    let mut db_config = match read_db_config("db-compose.toml") {
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
                            quick_links: Some(Some(serde_json::to_string(&db.links).unwrap())),
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
                            quick_links: Some(Some(serde_json::to_string(&db.links).unwrap())),
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

    write_db_config("db-compose.toml", &db_config).unwrap();
}

pub async fn register_package(
    package_path: &Path,
    metadata_config: &MetadataConfiguration,
    config_path: &Path,
    env: Environment,
    releaser_path: &Path,
) {
    let metadata_details = read_package_metadata_file(package_path).unwrap();

    let links_str = serde_json::to_string(&metadata_details.links).unwrap();

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
            LANG::Shell => todo!(),
        };

    println!("{:?} {:?}", releaser_config, version);

    dependencies_list.extend(internal_dependencies);

    let req_body = CreateOrUpdatePackageRequest {
        identifier: name,
        package_type: metadata_details.package_type,
        lang: metadata_details.lang.to_string(),
        version,
        organization_id: organization,
        description,
        dependencies: dependencies_list,
        env: env.to_string(),
        repo_origin: Some(releaser_config.settings.git_url_prefix),
        quick_links: Some(Some(links_str)),
    };
    println!("Request body: {:?}", req_body);

    match metadata_create_or_update_package(
        metadata_config,
        MetadataCreateOrUpdatePackageParams {
            create_or_update_package_request: req_body,
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
fn find_pipelines_to_trigger(
    dependencies_map: &HashMap<String, Vec<String>>,
    pivot_package: &String,
    triggered_set: &mut HashSet<String>,
) -> Vec<String> {
    let mut result = Vec::new();

    // Find all packages that directly depend on the pivot_package
    for (package, dependencies) in dependencies_map {
        if dependencies.contains(pivot_package) {
            // Check if any of the dependencies of this package are already in the triggered set
            let has_triggered_dependency =
                dependencies.iter().any(|dep| triggered_set.contains(dep));

            // Only add the package if none of its dependencies are in the triggered set
            if !has_triggered_dependency {
                // Check recursively if we can add this package
                let sub_dependencies =
                    find_pipelines_to_trigger(dependencies_map, package, triggered_set);

                // Add the current package to the result
                result.push(package.clone());

                // Add the sub-dependencies to the result
                result.extend(sub_dependencies);
            }
        }
    }

    // Add the pivot_package to the triggered set
    triggered_set.insert(pivot_package.clone());

    // Filter out packages that have dependencies which are already in the result
    let filtered_result: Vec<String> = result
        .clone()
        .into_iter()
        .filter(|pkg| {
            let a = &vec![];
            let deps = dependencies_map.get(pkg).unwrap_or(a);
            !deps.iter().any(|dep| result.contains(dep))
        })
        .collect();

    // Return the filtered result without duplicates
    filtered_result
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

pub async fn refresh_internal_dependency_versions(
    config_path: &Path,
    metadata_config: &MetadataConfiguration,
    releaser_path: &Path,
    package_path: &Path,
) {
    let config = read_service_config_file(config_path).unwrap();

    let (mut current_package_name, version, description, organization, internal_dependencies) =
        match config.lang {
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

    println!(
        "org , {} , {} , {:?}",
        config.organization_id, current_package_name, internal_dependencies
    );

    for dependency in internal_dependencies {
        println!("{:?}", dependency);

        let (org, pkg) = extract_org_and_package(&dependency).unwrap();
        match metadata_get_package_version(
            &metadata_config,
            MetadataGetPackageVersionParams {
                org_id: org,
                package_name: pkg.clone(),
            },
        )
        .await
        {
            Ok(new_version) => {
                match config.lang {
                    LANG::Python => update_python_internal_dependency(
                        &pkg,
                        &new_version.version,
                        &config.organization_id,
                    ),
                    LANG::TS => {
                        // Run the pnpm add command to update the dependency
                        let pnpm_command =
                            format!("pnpm add {}@{}", dependency, new_version.version);

                        let output = Command::new("sh")
                            .arg("-c")
                            .arg(&pnpm_command)
                            .output()
                            .expect("Failed to run pnpm add command");

                        if !output.status.success() {
                            eprintln!(
                                "Failed to update TypeScript dependency: {}. Error: {}",
                                dependency,
                                String::from_utf8_lossy(&output.stderr)
                            );
                        } else {
                            println!(
                                "Successfully updated TypeScript dependency: @{}{} to version {}.",
                                config.organization_id, dependency, new_version.version
                            );
                        }
                    }
                    LANG::Rust => {
                        // Run the cargo add command to update the Rust dependency
                        let cargo_command = format!("cargo add {}@{}", pkg, new_version.version);

                        let output = Command::new("sh")
                            .arg("-c")
                            .arg(&cargo_command)
                            .output()
                            .expect("Failed to run cargo add command");

                        if !output.status.success() {
                            eprintln!(
                                "Failed to update Rust dependency: {}. Error: {}",
                                dependency,
                                String::from_utf8_lossy(&output.stderr)
                            );
                        } else {
                            println!(
                                "Successfully updated Rust dependency: {} to version {}.",
                                dependency, new_version.version
                            );
                        }
                    }
                    LANG::Shell => todo!(),
                }
            }
            Err(e) => {
                println!("{:?}", e);
            }
        }
    }
}

pub async fn fetch_dependent_pipelines(
    config_path: &Path,
    iam_config: &IAMConfiguration,
    metadata_config: &MetadataConfiguration,
    pipeline_token: &String,
) {
    let config = read_service_config_file(config_path).unwrap();

    let (mut current_package_name, version, description, organization, internal_dependencies) =
        match config.lang {
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

    println!(
        "org , {} , {}",
        config.organization_id, current_package_name
    );

    let mut dependencies_map: HashMap<String, Vec<String>> = HashMap::new();

    let mut repo_details_map: HashMap<String, Option<(String, String)>> = HashMap::new();

    let mut repo_type_map: HashMap<String, String> = HashMap::new();

    match metadata_get_user_packages(
        metadata_config,
        MetadataGetUserPackagesParams {
            org_id: config.organization_id.clone(),
            env: "stage".to_string(),
        },
    )
    .await
    {
        Ok(packages) => {
            for pkg in packages {
                let key = format!(
                    "@{}/{}",
                    config.organization_id.clone(),
                    pkg.identifier.clone()
                );

                repo_details_map.insert(
                    key.clone(),
                    extract_username_and_repo(&pkg.repo_origin.unwrap().unwrap()),
                );

                repo_type_map.insert(key.clone(), "package".to_string());

                dependencies_map.insert(key.clone(), pkg.dependencies.clone());
            }
        }
        Err(_) => todo!(),
    }

    match metadata_get_dbschemas_and_tables(
        metadata_config,
        MetadataGetDbschemasAndTablesParams {
            org_id: config.organization_id.clone(),
            env: "stage".to_string(),
        },
    )
    .await
    {
        Ok(schemas) => {
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
                    for service in services {
                        let mut dependencies = service.dependencies.clone();

                        for schema in schemas.clone() {
                            if schema.identifier == service.db_schema_id
                                || schema.identifier == service.message_queue_schema_id
                                || schema.identifier == service.cache_schema_id
                            {
                                dependencies.push(format!(
                                    "@{}/{}",
                                    config.organization_id.clone(),
                                    schema.name
                                ));
                            }
                        }

                        let key = format!(
                            "@{}/{}",
                            config.organization_id.clone(),
                            service.identifier.clone()
                        );

                        dependencies_map.insert(key.clone(), dependencies);

                        repo_details_map.insert(
                            key.clone(),
                            extract_username_and_repo(&service.repo_origin.unwrap().unwrap()),
                        );
                        repo_type_map.insert(key.clone(), "service".to_string());
                    }
                }
                Err(_) => todo!(),
            }
        }
        Err(_) => todo!(),
    }

    let mut triggered_set = HashSet::new();
    let pipelines = find_pipelines_to_trigger(
        &dependencies_map,
        &format!(
            "@{}/{}",
            config.organization_id.clone(),
            current_package_name.clone()
        ),
        &mut triggered_set,
    );

    println!("Pipelines : {:?}", pipelines);

    let client = Client::new();
    for pipeline in pipelines {
        if let Some((repo_owner, repo_name)) =
            repo_details_map.get(&pipeline).and_then(|x| x.clone())
        {
            // Set the request URL for dispatching the workflow
            let url = format!(
                "https://api.github.com/repos/{}/{}/actions/workflows/CI.yml/dispatches",
                repo_owner, repo_name
            );

            // Prepare the JSON body
            let body = json!({
                "ref": "main" // Specify the branch to trigger the workflow
            });

            // Make the POST request
            let response = client
                .post(&url)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "ginger-connector") // Added User-Agent header
                .header("Authorization", format!("Bearer {}", pipeline_token))
                .json(&body)
                .send()
                .await;

            let (org, pkg) = extract_org_and_package(&pipeline).unwrap();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    println!("Workflow dispatched for pipeline: {}", pipeline);
                    match metadata_update_pipeline_status(
                        &metadata_config,
                        MetadataUpdatePipelineStatusParams {
                            pipeline_status_update_request: {
                                PipelineStatusUpdateRequest {
                                    env: "stage".to_string(),
                                    status: "waiting".to_string(),
                                    update_type: repo_type_map.get(&pipeline).unwrap().to_string(),
                                    org_id: org,
                                    identifier: pkg,
                                }
                            },
                        },
                    )
                    .await
                    {
                        Ok(status) => {
                            println!("{:?}", status);
                            sleep(Duration::from_secs(5)).await;
                        }
                        Err(e) => {
                            println!("Error calling metadata_update_pipeline_status{:?}", e);
                        }
                    }
                }
                Ok(resp) => {
                    eprintln!(
                        "Failed to dispatch workflow for pipeline {}: Status Code: {} {:?}",
                        pipeline,
                        resp.status(),
                        resp
                    );
                }
                Err(e) => eprintln!(
                    "Error occurred while dispatching workflow for pipeline {}: {:?}",
                    pipeline, e
                ),
            }
        } else {
            eprintln!("Repo details not found for pipeline: {}", pipeline);
        }
    }
}

fn extract_username_and_repo(github_url: &str) -> Option<(String, String)> {
    // Check if the input string starts with the expected GitHub URL pattern
    if github_url.starts_with("https://github.com/") {
        // Strip the "https://github.com/" part and split the rest by '/'
        let parts: Vec<&str> = github_url["https://github.com/".len()..]
            .split('/')
            .collect();

        // Ensure there are exactly two parts: username and repo name
        if parts.len() == 2 {
            let username = parts[0].to_string();
            let repo_name = parts[1].to_string();
            return Some((username, repo_name));
        }
    }
    // Return None if the URL is not valid or doesn't match the expected pattern
    None
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
                LANG::Shell => todo!(),
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

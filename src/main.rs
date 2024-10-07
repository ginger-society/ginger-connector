use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::{Parser, Subcommand};
use generate::generate_arbitrary_client;
use ginger_shared_rs::utils::split_slug;
use ginger_shared_rs::{Environment, LANG};
use init::initialize;
use publish::publish_metadata;
use serde_json::Value;
use service::{generate_client, generate_references};
use utils::{
    fetch_dependent_pipelines, fetch_metadata_and_process, refresh_internal_dependency_versions,
    register_db, register_package, system_check, update_pipeline,
};
use IAMService::apis::configuration::Configuration as IAMConfiguration;
use IAMService::apis::default_api::identity_validate_api_token;
use IAMService::get_configuration as get_iam_configuration;
use MetadataService::apis::default_api::{
    metadata_update_db_pipeline, MetadataUpdateDbPipelineParams,
};
use MetadataService::models::UpdateDbPipelineRequest;
use MetadataService::{
    apis::configuration::Configuration as MetadataConfiguration,
    get_configuration as get_metadata_configuration,
};

mod file_utils;
mod generate;
mod init;
mod publish;
mod refresher;
mod service;
mod utils;

/// Command line interface for managing the application
#[derive(Parser)]
#[clap(name = "CLI")]
#[clap(about = "A CLI for managing service dependencies", long_about = None)]
struct CLI {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch metadata and process it
    Init,
    /// publishes the project metadata to the metadata service
    Publish {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
    /// Register a package
    Register {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
    /// Configures a service to a project
    Config,
    /// Configures a service to a project
    SystemCheck {
        pipeline_token: String,
    },
    /// Configures a service to a project
    TriggerDependentPipelines {
        pipeline_token: String,
    },
    /// Connect to an environment
    Connect {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
    UpdatePipeline {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
        #[clap(value_parser)]
        status: String,
    },
    UpdateDBPipeline {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
        #[clap(value_parser)]
        status: String,
        #[clap(value_parser)]
        slug: String,
    },
    /// Generates references to portals
    Refer {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
    /// Generate a client for a specified language
    Generate {
        #[clap(value_enum)]
        lang: LANG,
        #[clap(value_parser)]
        swagger_path: String,
        #[clap(value_parser)]
        server_url: String,
        #[clap(value_parser)]
        out_folder: String,
    },
    Refresh,
}

#[tokio::main]
async fn check_session_gurad(
    cli: CLI,
    config_path: &Path,
    iam_config: &IAMConfiguration,
    metadata_config: &MetadataConfiguration,
    package_path: &Path,
    releaser_path: &Path,
) {
    match identity_validate_api_token(&iam_config).await {
        Ok(response) => {
            match &cli.command {
                Commands::SystemCheck { pipeline_token } => {
                    system_check(config_path, &iam_config, &metadata_config, pipeline_token).await
                }
                Commands::Refresh => {
                    refresh_internal_dependency_versions(config_path, &metadata_config).await
                }
                Commands::TriggerDependentPipelines { pipeline_token } => {
                    fetch_dependent_pipelines(
                        config_path,
                        &iam_config,
                        &metadata_config,
                        pipeline_token,
                    )
                    .await;
                }
                Commands::Config {} => {
                    fetch_metadata_and_process(config_path, &iam_config, &metadata_config).await;
                }
                Commands::Register { env } => {
                    if !Path::new("db-compose.toml").exists() {
                        println!("db-compose.toml not found. Running the register command.");
                        register_package(
                            package_path,
                            &metadata_config,
                            config_path,
                            env.clone(),
                            releaser_path,
                        )
                        .await
                    } else {
                        register_db(&metadata_config, releaser_path).await;
                    }
                }
                Commands::Connect { env } => {
                    generate_client(config_path, env.clone(), metadata_config).await
                }
                Commands::Refer { env } => generate_references(config_path, env.clone()),
                Commands::Init => initialize(config_path),
                Commands::Generate {
                    lang,
                    swagger_path,
                    server_url,
                    out_folder,
                } => {
                    generate_arbitrary_client(swagger_path, lang.clone(), server_url, out_folder);
                }
                Commands::Publish { env } => {
                    publish_metadata(
                        config_path,
                        env.clone(),
                        metadata_config,
                        releaser_path,
                        package_path,
                    )
                    .await
                }
                Commands::UpdatePipeline { env, status } => {
                    update_pipeline(
                        package_path,
                        &metadata_config,
                        config_path,
                        env.clone(),
                        status.clone(),
                    )
                    .await
                }
                Commands::UpdateDBPipeline { env, status, slug } => {
                    if let Some((org_id, name)) = split_slug(slug) {
                        println!("Organization ID: {}", org_id);
                        println!("Name: {}", name);
                        match metadata_update_db_pipeline(
                            &metadata_config,
                            MetadataUpdateDbPipelineParams {
                                update_db_pipeline_request: UpdateDbPipelineRequest {
                                    status: status.to_string(),
                                },
                                org_id: org_id,
                                schema_name: name,
                                branch_name: env.to_string(),
                            },
                        )
                        .await
                        {
                            Ok(resp) => {
                                println!("{:?}", resp);
                            }
                            Err(e) => {
                                println!("Error {}", e);
                            }
                        }
                    } else {
                        println!("Invalid slug format");
                    }
                }
            };

            println!("Token is valid: {:?}", response)
        }
        Err(error) => {
            println!("Token validation failed: {:?}", error);
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = CLI::parse();

    let config_path = Path::new("services.toml");
    let package_path = Path::new("metadata.toml");
    let releaser_path = Path::new("releaser.toml");

    let home_dir = match dirs::home_dir() {
        Some(path) => path,
        None => {
            println!("Failed to locate home directory. Exiting.");
            exit(1);
        }
    };

    // Construct the path to the auth.json file
    let auth_file_path: PathBuf = [home_dir.to_str().unwrap(), ".ginger-society", "auth.json"]
        .iter()
        .collect();

    // Read the token from the file
    let mut file = match File::open(&auth_file_path) {
        Ok(f) => f,
        Err(_) => {
            println!("Failed to open {}. Exiting.", auth_file_path.display());
            exit(1);
        }
    };
    let mut contents = String::new();
    if let Err(_) = file.read_to_string(&mut contents) {
        println!("Failed to read the auth.json file. Exiting.");
        exit(1);
    }

    let json: Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(_) => {
            println!("Failed to parse auth.json as JSON. Exiting.");
            exit(1);
        }
    };

    let token = match json.get("API_TOKEN").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            println!("API_TOKEN not found in auth.json. Exiting.");
            exit(1);
        }
    };

    let iam_config: IAMConfiguration = get_iam_configuration(Some(token.clone()));
    println!("{:?}", token);
    let metadata_config: MetadataConfiguration = get_metadata_configuration(Some(token.clone()));

    check_session_gurad(
        cli,
        config_path,
        &iam_config,
        &metadata_config,
        package_path,
        releaser_path,
    );
}

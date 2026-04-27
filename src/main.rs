use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use generate::generate_arbitrary_client;
use ginger_shared_rs::utils::{get_token_from_file_storage, split_slug};
use ginger_shared_rs::{Environment, LANG};
use init::initialize;
use publish::publish_metadata;
use serde_json::Value;
use service::{generate_client, generate_references};
use utils::{
    fetch_dependent_pipelines, fetch_metadata_and_process, gen_ist,
    refresh_internal_dependency_versions, register_db, register_package, system_check,
    trigger_pipeline, update_pipeline, WatchContent,
};
use tokio::signal;

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
use tokio_tungstenite::connect_async;
use futures_util::{stream::StreamExt, SinkExt};


mod file_utils;
mod generate;
mod init;
mod publish;
mod refresher;
mod service;
mod utils;
mod ws_handler;


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
    /// This triggers the lowest set of components with zero depedencies. This will bubble up to run everything once again
    SystemCheck { pipeline_token: String },
    /// Given the JWT secret , this generates a long live token that can be used to call inter service endpoints
    GenIST { jwt_secret: String },
    /// Finds out and triggers the dependent pipelines
    TriggerDependentPipelines {
        pipeline_token: String,
        #[clap(short, long, use_value_delimiter = true)]
        pipelines_to_skip: Option<Vec<String>>,
    },
    /// Finds out and triggers the dependent pipelines
    TriggerPipeline { id: String, pipeline_token: String },
    /// Connect to an environment and generate the client
    Connect {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
    /// this updates the pipeline statuses for components except for the DBs
    UpdatePipeline {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
        #[clap(value_parser)]
        status: String,
    },
    /// this updates the DBs pipeline status
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
    /// This updates all the internal dependencies packages versions, should be run in dev machine for a sanity test and then also in the pipelien
    Refresh,
    Watch {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
}

// Update the handle_ws_event function signature to accept owned values
async fn handle_ws_event_async(service_name: String, metadata_config: MetadataConfiguration, config_path: PathBuf) {
    // Call the original handler with references
    ws_handler::handle_ws_event(&service_name, &metadata_config, &config_path).await;
}

async fn start_websocket_watcher(metadata_config: MetadataConfiguration, config_path: PathBuf) {
    let token = get_token_from_file_storage();

    // Token validation is done once at the beginning
    let response = match identity_validate_api_token(&get_iam_configuration(Some(token.clone()))).await {
        Ok(res) => res,
        Err(error) => {
            println!("Token validation failed: {:?}", error);
            exit(1);
        }
    };

    let url = format!(
        "wss://api.gingersociety.org/notification/ws/workspace_{}?token={}",
        response.sub, token
    );

    let mut attempt: u32 = 0;
    loop {
        println!("Attempting to connect to WebSocket (try #{})...", attempt + 1);

        match connect_async(&url).await {
            Ok((mut ws_stream, _)) => {
                println!("WebSocket connection established");

                while let Some(msg_result) = ws_stream.next().await {
                    match msg_result {
                        Ok(msg) => {
                            if msg.is_text() {
                                let content = msg.into_text().unwrap_or_default();

                                match serde_json::from_str::<WatchContent>(&content) {
                                    Ok(watch_content) => {
                                        println!("Received event: {:?}", watch_content);


                                        if watch_content.event.trim().eq_ignore_ascii_case("CONNECT") {
                                            handle_ws_event_async(
                                                watch_content.resource_id.trim().to_string(),
                                                metadata_config.clone(),
                                                config_path.clone(),
                                            ).await;
                                        }else {
                                            println!("⚠️ Unhandled event type: {}", watch_content.event);
                                        }

                                       
                                    }
                                    Err(_) => println!("💬 Non-JSON WS message: {}", content),
                                }


                                
                            }
                        }
                        Err(e) => {
                            eprintln!("WebSocket error: {:?}. Reconnecting...", e);
                            break; // Exit the inner loop to retry connection
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to connect to WebSocket: {:?}", e);
            }
        }

        attempt += 1;
        let backoff = std::cmp::min(60, 2_u64.pow(attempt)); // max 60s
        println!("Reconnecting in {} seconds...", backoff);
        tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
    }
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
            // Process the CLI command
            match &cli.command {
                Commands::Watch { env } => {
                    
                    generate_client(config_path, env.clone(), metadata_config).await;
                    
                    // WebSocket connection is triggered only when running `watch`


                    start_websocket_watcher(metadata_config.clone(), config_path.to_path_buf()).await;
                }
                Commands::TriggerPipeline { id, pipeline_token } => {
                    println!("{:?} , {:?}", pipeline_token, id);
                    trigger_pipeline(
                        config_path,
                        &iam_config,
                        &metadata_config,
                        pipeline_token,
                        id,
                    )
                    .await;
                }
                Commands::GenIST { jwt_secret } => gen_ist(package_path, jwt_secret),
                Commands::SystemCheck { pipeline_token } => {
                    system_check(config_path, &iam_config, &metadata_config, pipeline_token).await
                }
                Commands::Refresh => {
                    refresh_internal_dependency_versions(config_path, &metadata_config).await
                }
                Commands::TriggerDependentPipelines {
                    pipeline_token,
                    pipelines_to_skip,
                } => {
                    let pipeline_ids_to_skip = pipelines_to_skip.clone().unwrap_or_else(Vec::new);
                    println!("{:?}", pipeline_ids_to_skip);
                    fetch_dependent_pipelines(
                        config_path,
                        &iam_config,
                        &metadata_config,
                        pipeline_token,
                        pipeline_ids_to_skip,
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

    let token = get_token_from_file_storage();

    let iam_config: IAMConfiguration = get_iam_configuration(Some(token.clone()));
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
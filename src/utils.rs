use std::{
    cmp::Ordering,
    collections::HashMap,
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::Path,
    process::{exit, Command},
};

use clap::ValueEnum;
use colored::Colorize;
use inquire::{list_option::ListOption, validator::Validation, MultiSelect};
use serde::{Deserialize, Serialize};
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

pub fn split_slug(slug: &str) -> Option<(String, String)> {
    // Attempt to split the slug into two parts based on the '/'
    match slug.split_once('/') {
        Some((org_id, name)) => Some((org_id.to_string(), name.to_string())),
        None => None, // Return None if the slug does not contain a '/'
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ORM {
    TypeORM,
    SQLAlchemy,
    DjangoORM,
    Diesel,
}

impl fmt::Display for ORM {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ORM::TypeORM => write!(f, "TypeORM"),
            ORM::SQLAlchemy => write!(f, "SQLAlchemy"),
            ORM::DjangoORM => write!(f, "DjangoORM"),
            ORM::Diesel => write!(f, "Diesel"),
        }
    }
}

#[derive(Deserialize, Debug, Serialize)]
pub struct DBSchema {
    pub url: String,
    pub lang: LANG,
    pub orm: ORM,
    pub root: String,
    pub schema_id: Option<String>,
    pub cache_schema_id: Option<String>,
    pub branch: Option<String>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct DBTables {
    pub names: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct DBConfig {
    pub schema: DBSchema,
    pub tables: DBTables,
}

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
            LANG::TS => write!(f, "typescript"),
            LANG::Python => write!(f, "python"),
        }
    }
}

impl LANG {
    pub fn all() -> Vec<LANG> {
        vec![LANG::Rust, LANG::TS, LANG::Python]
    }
}

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct Config {
    pub services: Option<HashMap<String, HashMap<String, String>>>,
    pub portals_refs: Option<HashMap<String, HashMap<String, String>>>,
    pub lang: LANG,
    pub organization_id: String,
    pub dir: Option<String>, // in case the project does not need any service integration
    pub portal_refs_file: Option<String>,
    pub spec_url: Option<String>,
    pub urls: Option<HashMap<String, String>>,
    pub override_name: Option<String>,
    pub service_type: Option<String>,
}

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct PackageMetadata {
    pub lang: LANG,
    pub package_type: String,
}

#[derive(Debug)]
pub enum FileType {
    Py,
    Toml,
    Json,
    Unknown,
}

impl FileType {
    fn from_extension(ext: Option<&str>) -> FileType {
        match ext {
            Some("py") => FileType::Py,
            Some("toml") => FileType::Toml,
            Some("json") => FileType::Json,
            _ => FileType::Unknown,
        }
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileType::Py => write!(f, "Py"),
            FileType::Toml => write!(f, "Toml"),
            FileType::Json => write!(f, "Json"),
            FileType::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub enum Channel {
    Final,
    Nightly, // Also known as Dev branch
    Alpha,
    Beta,
}
impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Channel::Nightly => write!(f, "nightly"),
            Channel::Final => write!(f, "final"),
            Channel::Alpha => write!(f, "alpha"),
            Channel::Beta => write!(f, "beta"),
        }
    }
}

impl From<&str> for Channel {
    fn from(channel: &str) -> Self {
        match channel {
            "nightly" => Channel::Nightly,
            "alpha" => Channel::Alpha,
            "beta" => Channel::Beta,
            "final" => Channel::Final,
            _ => exit(1),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Eq)]
pub struct Version {
    pub channel: Channel,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub revision: u32,
}

impl Version {
    pub fn formatted(&self) -> String {
        match &self.channel {
            Channel::Final => {
                format!("{}.{}.{}", self.major, self.minor, self.patch)
            }
            _ => {
                format!(
                    "{}.{}.{}-{}.{}",
                    self.major, self.minor, self.patch, self.channel, self.revision
                )
            }
        }
    }
    pub fn tuple(&self) -> String {
        format!(
            "({}, {}, {}, \"{}\", {})",
            self.major, self.minor, self.patch, self.channel, self.revision
        )
    }

    pub fn from_str(version: &str) -> Self {
        let parts: Vec<&str> = version.split(|c| c == '.' || c == '-').collect();
        let major = parts[0].parse().unwrap_or(0);
        let minor = parts[1].parse().unwrap_or(0);
        let patch = parts[2].parse().unwrap_or(0);
        let (channel, revision) = if parts.len() > 3 {
            (Channel::from(parts[3]), parts[4].parse().unwrap_or(0))
        } else {
            (Channel::Final, 0)
        };

        Version {
            major,
            minor,
            patch,
            channel,
            revision,
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(self.channel.cmp(&other.channel))
            .then(self.revision.cmp(&other.revision))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.channel == other.channel
            && self.revision == other.revision
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum OutputType {
    String,
    Tuple,
}

impl fmt::Display for OutputType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputType::String => write!(f, "String"),
            OutputType::Tuple => write!(f, "Tuple"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Reference {
    pub file_name: String,
    #[serde(default = "default_output_type")] // Use a default value function
    pub output_type: OutputType, // `type` is a reserved keyword in Rust
    pub variable: String,
    #[serde(skip, default = "default_file_type")] // This field is not in the TOML file
    pub file_type: FileType,
}

fn default_file_type() -> FileType {
    FileType::Unknown
}

fn default_output_type() -> OutputType {
    OutputType::String // Default value is "string"
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ReleaserSettings {
    pub git_url_prefix: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ReleaserConfig {
    pub settings: ReleaserSettings,
    pub version: Version,
}

pub fn read_releaser_config_file<P: AsRef<Path>>(
    file_path: P,
) -> Result<ReleaserConfig, Box<dyn std::error::Error>> {
    // Read the file content into a string
    let contents = fs::read_to_string(file_path)?;

    // Parse the TOML string into the Settings struct
    let settings: ReleaserConfig = toml::de::from_str(&contents)?;

    Ok(settings)
}

pub fn read_config_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn read_package_metadata_file<P: AsRef<Path>>(
    path: P,
) -> Result<PackageMetadata, Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let config: PackageMetadata = toml::from_str(&content)?;
    Ok(config)
}

pub fn write_config_file<P: AsRef<Path>>(path: P, config: &Config) -> Result<(), Box<dyn Error>> {
    let content = toml::to_string(config)?;
    fs::write(path, content)?;
    Ok(())
}

fn get_internal_dependencies<P: AsRef<Path>>(
    file_path: P,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let json_value: Value = serde_json::from_reader(reader)?;

    // Extract the prefix from the "name" field
    let prefix = if let Some(name) = json_value.get("name").and_then(|n| n.as_str()) {
        if let Some(pos) = name.rfind('/') {
            &name[..=pos]
        } else {
            name
        }
    } else {
        return Err("Unable to find the 'name' field in the file".into());
    };

    let mut internal_dependencies = Vec::new();

    if let Some(dependencies) = json_value.get("dependencies").and_then(|d| d.as_object()) {
        for (key, _) in dependencies {
            if key.starts_with(prefix) {
                internal_dependencies.push(key.clone());
            }
        }
    }

    if let Some(dev_dependencies) = json_value
        .get("devDependencies")
        .and_then(|d| d.as_object())
    {
        for (key, _) in dev_dependencies {
            if key.starts_with(prefix) {
                internal_dependencies.push(key.clone());
            }
        }
    }

    Ok(internal_dependencies)
}

pub async fn update_pipeline(
    package_path: &Path,
    iam_config: &IAMConfiguration,
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
            Some(id) => {
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
                        schema_id: db.id.clone().unwrap(),
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
            None => {
                println!("Database '{}' is missing an ID", db.name);
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
                        // println!("Success : {:?}", resp.identifier)
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
    let mut config = read_config_file(config_path).unwrap();
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

            match write_config_file(config_path, &config) {
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

pub fn read_db_config<P: AsRef<Path>>(path: P) -> Result<DBConfig, Box<dyn Error>> {
    // Open the file
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    // Deserialize the TOML contents into the DBConfig struct
    let config: DBConfig = toml::from_str(&contents)?;
    Ok(config)
}

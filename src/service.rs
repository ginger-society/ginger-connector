use std::{
    fs::{self, read_to_string, write, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{exit, Command},
};

use colored::Colorize;
use ginger_shared_rs::{read_service_config_file, Service, LANG};
use MetadataService::apis::configuration::Configuration as MetadataConfiguration;
use MetadataService::apis::default_api::{
    metadata_get_service_and_env_by_id, MetadataGetServiceAndEnvByIdParams,
};

use crate::{file_utils::replace_in_file, Environment};

fn replace_in_files_recursive(dir_path: &str, pattern: &str, replacement: &str) -> io::Result<()> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        print!("Replacing import statements in : {:?}\n", path);
        if path.is_dir() {
            // Recurse into subdirectory
            replace_in_files_recursive(path.to_str().unwrap(), pattern, replacement)?;
        } else if path.is_file() {
            // Perform replacement in the file
            replace_in_file(path.to_str().unwrap(), pattern, replacement)?;
        }
    }
    Ok(())
}
fn open_api_client_generator(service: &Service, lang: LANG, root_dir: &str, base_url: &str) {
    let output_dir = format!("{}/{}_client", root_dir, service.name);
    println!("Generating client for: {:?}", service);

    let language = match lang {
        LANG::TS => String::from("typescript-fetch"),
        LANG::Rust => String::from("rust"),
        LANG::Python => String::from("python"),
        _ => lang.to_string(),
    };

    let mut binding = Command::new("openapi-generator-cli");
    let command = binding
        .arg("generate")
        .arg("-g")
        .arg(language)
        .arg("-o")
        .arg(&output_dir)
        .arg("--additional-properties")
        .arg(format!(
            "useSingleRequestParameter=true,packageName={}",
            service.name
        ))
        .arg("-i")
        .arg(service.schema_url.clone());

    // Add library option for Python
    match lang {
        LANG::Python => {
            command.arg("--library").arg("urllib3"); // Specify the library to use
        }
        _ => {}
    }

    let output = command.output();

    match output {
        Ok(cmd_output) => {
            for line in String::from_utf8(cmd_output.stdout)
                .unwrap()
                .split('\n')
                .into_iter()
            {
                println!("O: {}", line)
            }

            match lang {
                LANG::Shell => todo!(),
                LANG::Rust => {
                    println!(
                        "please add \n\n{} = {{ path = \"{}\" }}\n\n in cargo.toml file if not added",
                        service.name, output_dir
                    );

                    // Update lib.rs file
                    let lib_rs_path = format!("{}/src/lib.rs", output_dir);
                    let mut lib_rs_content = "".to_string();

                    if Path::new("Rocket.toml").exists() {
                        lib_rs_content = format!(
                            r#"
    use apis::configuration::Configuration;
    
    pub fn get_configuration() -> Configuration {{
        let config = Configuration {{
            base_path: "{url}".to_string(),
            ..Default::default()
        }};
        config
    }}
    "#,
                            url = base_url
                        );
                    } else {
                        lib_rs_content = format!(
                            r#"
    
    use std::{{env, process::exit}};
    
    use apis::configuration::{{ApiKey, Configuration}};
    
    pub fn get_configuration(token_arg: Option<String>) -> Configuration {{
        let token = match token_arg {{
            Some(t) => t,

            None => env::var("GINGER_API_TOKEN").unwrap_or_else(|_| {{
                println!("GINGER_API_TOKEN environment variable not set. Exiting.");
                exit(1)
            }}),
        }};
        let config = Configuration {{
            base_path: "{url}".to_string(),
            api_key: Some(ApiKey {{
                key: token,
                prefix: Some("".to_string()),
            }}),
            ..Default::default()
        }};
        config
    }}
    pub fn get_configuration_without_auth() -> Configuration {{
        let config = Configuration {{
            base_path: "{url}".to_string(),
            ..Default::default()
        }};
        config
    }}
    "#,
                            url = base_url
                        );
                    }

                    let config_file_content = format!(
                        r#"
                        use okapi::openapi3::{{Object, SecurityRequirement, SecurityScheme, SecuritySchemeData}};
use rocket::http::Status;
use rocket::request::{{FromRequest, Outcome, Request}};
use rocket_okapi::gen::OpenApiGenerator;
use rocket_okapi::request::{{OpenApiFromRequest, RequestHeaderInput}};
use {name}::apis::configuration::{{ApiKey, Configuration}}; // Adjust based on your crate structure
use {name}::get_configuration; // Assuming get_configuration exists and returns Configuration

#[derive(Debug)]
pub struct {name}_config(pub Configuration); // Wrapper struct for Configuration

#[rocket::async_trait]
impl<'r> FromRequest<'r> for {name}_config {{
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {{
        let keys: Vec<_> = request.headers().get("Authorization").collect();
        if keys.len() != 1 {{
            return Outcome::Error((Status::Unauthorized, ()));
        }}

        let token_str = keys[0].trim_start_matches("Bearer ").trim().to_string();
        let mut configuration = get_configuration(); // Assuming Configuration::new or get_configuration exists

        // Assuming Configuration has a method to set api_key
        configuration.api_key = Some(ApiKey {{
            key: token_str,
            prefix: None,
        }});

        Outcome::Success({name}_config(configuration))
    }}
}}

impl<'a> OpenApiFromRequest<'a> for {name}_config {{
    fn from_request_input(
        _gen: &mut OpenApiGenerator,
        _name: String,
        _required: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {{
        let security_scheme = SecurityScheme {{
            description: Some("Requires a Bearer token to access".to_owned()),
            data: SecuritySchemeData::ApiKey {{
                name: "Authorization".to_owned(),
                location: "header".to_owned(),
            }},
            extensions: Object::default(),
        }};

        let mut security_req = SecurityRequirement::new();
        security_req.insert("BearerAuth".to_owned(), Vec::new());

        Ok(RequestHeaderInput::Security(
            "BearerAuth".to_owned(),
            security_scheme,
            security_req,
        ))
    }}

    fn get_responses(
        _gen: &mut rocket_okapi::gen::OpenApiGenerator,
    ) -> rocket_okapi::Result<okapi::openapi3::Responses> {{
        Ok(okapi::openapi3::Responses::default())
    }}
}}
"#,
                        name = service.name
                    );

                    if Path::new("Rocket.toml").exists() {
                        match OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(format!("src/middlewares/{}_config.rs", service.name))
                        {
                            Ok(mut file) => {
                                if let Err(e) = file.write_all(config_file_content.as_bytes()) {
                                    eprintln!("Error writing to config.rs: {:?}", e);
                                }
                            }
                            Err(e) => eprintln!("Error creating config.rs: {:?}", e),
                        }
                    }

                    match OpenOptions::new()
                        .write(true)
                        .append(true)
                        .open(&lib_rs_path)
                    {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(lib_rs_content.as_bytes()) {
                                eprintln!("Error writing to lib.rs: {:?}", e);
                            }
                        }
                        Err(e) => eprintln!("Error opening lib.rs: {:?}", e),
                    }
                }
                LANG::TS => {
                    // Add content to index.ts
                    let index_ts_path = format!("{}/index.ts", output_dir);
                    let index_ts_content = format!(
                        r#"/* tslint:disable */
/* eslint-disable */

import {{ DefaultApi }} from './apis'
import {{ Configuration }} from './runtime'

export * from './runtime';
export * from './apis/index';
export * from './models/index';
const getToken = (): string | null => {{
    return localStorage.getItem('access_token'); // Adjust the key name as needed
}};
const configuration = new Configuration({{
    basePath: '{}',
    middleware: [
        {{
            pre: async (context) => {{
                const token = getToken();
                if (token) {{
                    context.init.headers = {{
                        ...context.init.headers,
                        Authorization: token,
                    }};
                }}
                return Promise.resolve(context);
            }},
        }},
    ],
}})
const client = new DefaultApi(configuration)
export default client
"#,
                        base_url
                    );

                    match OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&index_ts_path)
                    {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(index_ts_content.as_bytes()) {
                                eprintln!("Error writing to index.ts: {:?}", e);
                            }
                        }
                        Err(e) => eprintln!("Error creating index.ts: {:?}", e),
                    }
                }
                LANG::Python => {
                    // TODO
                    // update

                    let file_path_1 = &format!("{}/{}/__init__.py", output_dir, service.name);
                    println!("{}", file_path_1);
                    replace_in_file(
                        file_path_1,
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    replace_in_file(
                        &format!("{}/{}/api/__init__.py", output_dir, service.name),
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    replace_in_file(
                        &format!("{}/{}/api/default_api.py", output_dir, service.name),
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    replace_in_files_recursive(
                        &format!("{}/{}/models", output_dir, service.name),
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    replace_in_file(
                        &format!("{}/{}/api_client.py", output_dir, service.name),
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    // import IAMService.models
                    // from IAMService import rest
                    println!("{}", format!("import {}.models", service.name));
                    match replace_in_file(
                        &format!("{}/{}/api_client.py", output_dir, service.name),
                        &format!("import {}.models", service.name),
                        &format!("import {}_client.{}.models", service.name, service.name),
                    ) {
                        Ok(_) => {
                            match replace_in_file(
                                &format!("{}/{}/api_client.py", output_dir, service.name),
                                &format!("({}.models", service.name),
                                &format!("({}_client.{}.models", service.name, service.name),
                            ) {
                                Ok(_) => {
                                    replace_in_file(
                                        &format!("{}/{}/api_client.py", output_dir, service.name),
                                        &format!("from {} import rest", service.name),
                                        &format!(
                                            "from {}_client.{} import rest",
                                            service.name, service.name
                                        ),
                                    )
                                    .unwrap();
                                }
                                Err(_) => {}
                            };
                        }
                        Err(_) => {}
                    };

                    replace_in_file(
                        &format!("{}/{}/rest.py", output_dir, service.name),
                        &format!("from {}.", service.name),
                        &format!("from {}_client.{}.", service.name, service.name),
                    )
                    .unwrap();

                    let config_file_content = format!(
                        r#"
import certifi
from IAMService_client.IAMService import (
    Configuration,
)


def get_configuration(access_token):

    if access_token is None:
        return Configuration(
            host="{url}",
            ssl_ca_cert=certifi.where(),
        )
    else:
        return Configuration(
            host="{url}",
            ssl_ca_cert=certifi.where(),
            api_key={{
                "BearerAuth": access_token
            }},  # Set the access token with 'BearerAuth' identifier
            api_key_prefix={{"BearerAuth": "Bearer"}},
        )


def get_api_configuration(access_token):

    if access_token is None:
        return Configuration(
            host="{url}",
            ssl_ca_cert=certifi.where(),
        )
    else:
        return Configuration(
            host="{url}",
            ssl_ca_cert=certifi.where(),
            api_key={{
                "BearerAPIAuth": access_token
            }},  # Set the access token with 'BearerAuth' identifier
            api_key_prefix={{"BearerAPIAuth": "Bearer"}},
        )
                        "#,
                        url = base_url
                    );

                    match OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(format!("{}/{}/config_utils.py", output_dir, service.name))
                    {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(config_file_content.as_bytes()) {
                                eprintln!("Error writing to config_utils.py: {:?}", e);
                            }
                        }
                        Err(e) => eprintln!("Error config_utils.py: {:?}", e),
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("{:?}", err)
        }
    }
}

fn print_openapi_generator_not_found() {
    println!(
        "The OpenAPI generator is not installed on your machine. Please use {} on MacOS / Windows / Linux",
        "npm install @openapitools/openapi-generator-cli -g".green()
    );
    exit(1);
}

pub fn generate_references(config_path: &Path, env: Environment) {
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

    // Process services and generate references content
    let mut references_content = match services_config.lang {
        LANG::TS => {
            format!("export const ENV_KEY=\"{env}\";\n")
        }
        LANG::Rust => todo!(),
        LANG::Python => {
            format!("ENV_KEY=\"{env}\"\n")
        }
        LANG::Shell => todo!(),
    };

    // Ensure portal_refs_file is specified in the configuration
    let refs_file = match &services_config.refs_file {
        Some(file) => file,
        None => {
            println!("'portal_refs_file' is not specified in the configuration. Exiting.");
            exit(1);
        }
    };

    // Process portals_refs if available
    if let Some(portals_refs) = &services_config.portals_refs {
        for (portal_name, portal_envs) in portals_refs {
            if let Some(portal_url) = portal_envs.get(&env.to_string()) {
                let formatted_name = portal_name
                    .replace("-", "_")
                    .replace("@", "")
                    .replace("/", "_")
                    .to_uppercase();
                match services_config.lang {
                    LANG::TS => {
                        references_content.push_str(&format!(
                            "export const {} = '{}';\n",
                            formatted_name, portal_url
                        ));
                    }
                    LANG::Rust => todo!(),
                    LANG::Python => {
                        references_content
                            .push_str(&format!("{} = '{}'\n", formatted_name, portal_url));
                    }
                    LANG::Shell => todo!(),
                }
            }
        }
    }

    if let Some(ws_ref) = &services_config.ws_refs {
        for (ws_name, ws_envs) in ws_ref {
            if let Some(ws_url) = ws_envs.get(&env.to_string()) {
                let formatted_name = ws_name
                    .replace("-", "_")
                    .replace("@", "")
                    .replace("/", "_")
                    .to_uppercase();
                match services_config.lang {
                    LANG::TS => {
                        references_content.push_str(&format!(
                            "export const {}_WS = '{}';\n",
                            formatted_name, ws_url
                        ));
                    }
                    LANG::Rust => todo!(),
                    LANG::Python => {
                        references_content
                            .push_str(&format!("{} = '{}'\n", formatted_name, ws_url));
                    }
                    LANG::Shell => todo!(),
                }
            }
        }
    }

    // Write the references content to the specified file
    match File::create(refs_file) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(references_content.as_bytes()) {
                println!("Failed to write to file '{}': {:?}", refs_file, err);
                exit(1);
            }
        }
        Err(err) => {
            println!("Failed to create file '{}': {:?}", refs_file, err);
            exit(1);
        }
    }

    println!("References generated successfully in '{}'", refs_file);

    // Add portal_refs_file to .gitignore if it's not already present
    let gitignore_path = Path::new(".gitignore");
    let portal_refs_file_str = refs_file.as_str();

    if gitignore_path.exists() {
        let gitignore_content = fs::read_to_string(gitignore_path).unwrap();
        if !gitignore_content.contains(portal_refs_file_str) {
            let mut gitignore_file = OpenOptions::new()
                .append(true)
                .open(gitignore_path)
                .unwrap();
            writeln!(gitignore_file, "\n{}", portal_refs_file_str).unwrap();
            println!("Added '{}' to .gitignore", portal_refs_file_str);
        } else {
            println!("'{}' is already in .gitignore", portal_refs_file_str);
        }
    } else {
        let mut gitignore_file = File::create(gitignore_path).unwrap();
        writeln!(gitignore_file, "{}", portal_refs_file_str).unwrap();
        println!("Created .gitignore and added '{}'", portal_refs_file_str);
    }
}

fn extract_org_and_package(input: &str) -> Option<(String, String)> {
    // Check if the input starts with '@'
    if input.starts_with('@') {
        // Split the input on '/'
        let parts: Vec<&str> = input.splitn(2, '/').collect();
        if parts.len() == 2 {
            // Extract and return org_id and package_name
            let org_id = parts[0][1..].to_string(); // Remove the '@' character
            let package_name = parts[1].to_string();
            return Some((org_id, package_name));
        }
    }
    // Return None if the input is not in the expected format
    None
}

pub async fn generate_client(
    config_path: &Path,
    env: Environment,
    metadata_config: &MetadataConfiguration,
) {
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

    for (service_name, service_urls) in services_config.services.unwrap().iter() {
        let base_url = match env {
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
                    env: env.to_string(),
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
                        env
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
                        package_name, org_id, env
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
}

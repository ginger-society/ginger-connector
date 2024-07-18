use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::{exit, Command},
};

use colored::Colorize;

use crate::{
    utils::{read_config_file, Service, LANG},
    Environment,
};

fn open_api_client_generator(service: &Service, lang: LANG, root_dir: &str, base_url: &str) {
    let output_dir = format!("{}/{}_client", root_dir, service.name);
    println!("Generating client for: {}", service.name);

    let output = Command::new("openapi-generator")
        .arg("generate")
        .arg("-g")
        .arg(lang.to_string())
        .arg("-o")
        .arg(&output_dir)
        .arg("--additional-properties")
        .arg(format!(
            "useSingleRequestParameter=true,packageName={}",
            service.name
        ))
        .arg("-i")
        .arg(service.schema_url.clone())
        .output();

    match output {
        Ok(cmd_output) => {
            for line in String::from_utf8(cmd_output.stdout)
                .unwrap()
                .split('\n')
                .into_iter()
            {
                println!("{}", line)
            }

            match lang {
                LANG::Rust => {
                    println!("please add Service_client_name = {{ path = \"folder_name\" }} in cargo.toml file if not added");

                    // Update lib.rs file
                    let lib_rs_path = format!("{}/src/lib.rs", output_dir);
                    let lib_rs_content = format!(
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
                LANG::Python => todo!(),
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

pub fn generate_client(config_path: &Path, env: Environment) {
    match Command::new("java").arg("-version").output() {
        Ok(cmd_output) => {
            if !String::from_utf8(cmd_output.stderr)
                .unwrap()
                .contains("java version")
            {
                println!("Java is not installed on your machine. Please use https://www.java.com/en/download/help/download_options.html to install Java first");
                exit(1);
            } else {
                match Command::new("openapi-generator").arg("--version").output() {
                    Ok(cmd_output) => {
                        if !String::from_utf8(cmd_output.stdout)
                            .unwrap()
                            .contains("openapi-generator-cli")
                        {
                            print_openapi_generator_not_found();
                        }
                    }
                    Err(_) => {
                        print_openapi_generator_not_found();
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("{:?}", err)
        }
    }

    let services_config = match read_config_file(config_path) {
        Ok(c) => c,
        Err(_) => {
            println!(
                "There is no service configuration found. Please use {} to add one. Exiting",
                "ginger-connector init".blue()
            );
            exit(1);
        }
    };

    println!("{:?}", services_config);

    for (service_name, service_urls) in services_config.services.unwrap().iter() {
        let base_url = match env {
            Environment::Dev => service_urls["dev"].clone(),
            Environment::Stage => service_urls["stage"].clone(),
            Environment::Prod => service_urls["prod"].clone(),
        };

        open_api_client_generator(
            &Service {
                schema_url: format!(
                    "https://raw.githubusercontent.com/{}/main/{}/{}.json",
                    services_config.repo, service_name, env
                ),
                name: service_name.to_string(),
            },
            services_config.lang,
            &services_config.dir,
            &base_url,
        );
    }
}

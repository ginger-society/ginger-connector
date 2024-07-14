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
                LANG::Rust => todo!(),
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

const configuration = new Configuration({{
    basePath: '{}'
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

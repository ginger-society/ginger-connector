use std::{
    env,
    fmt::{self},
    fs::{self, File},
    io::Write,
    process::{exit, Command},
};

use colored::Colorize;
use inquire::Select;
use serde::{Deserialize, Serialize};

use crate::{generators::init::STAFFE_PACKAGE_KIND, utils};

#[derive(Deserialize, Debug, Clone, Serialize)]
struct Service {
    id: i32,
    name: String,
    prod_url: String,
    prod_schema_url: String,
    stage_url: String,
    stage_schema_url: String,
    auth_token_env_key: String,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
struct ServicesConfig {
    schema: String,
    services: Vec<Service>,
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn read_services_config_json() -> Result<ServicesConfig, Box<dyn std::error::Error>> {
    match fs::read_to_string("services.config.json") {
        Ok(c) => {
            let services_config: ServicesConfig = match serde_json::from_str(&c) {
                Ok(c) => c,
                Err(error) => {
                    println!("services.config.json is invalid : `{}`", error);
                    exit(1);
                }
            };
            return Ok(services_config);
        }
        Err(error) => {
            eprintln!(
                "Could not read file `{}` , `{}`",
                "services.config.json", error
            );
            return Err(Box::new(error));
        }
    };
}

fn write_services_config_json(config: ServicesConfig) {
    let config_str = match serde_json::to_string_pretty(&config) {
        Ok(c) => c,
        Err(_) => {
            println!("Invalid JSON recieved.");
            exit(1)
        }
    };
    let service_config_file_name = String::from("services.config.json");

    match File::create(&service_config_file_name) {
        Ok(mut c) => {
            match c.write_all(config_str.as_bytes()) {
                Ok(_) => println!("Yayyy!. Success"),
                Err(err) => eprintln!("Unable to write service config file : {}", err),
            };
        }
        Err(_) => {
            eprintln!(
                "Unable to write config to {} file, exiting",
                service_config_file_name
            );
            exit(1)
        }
    };
}

#[tokio::main]
pub async fn add_service(openapi_config: Configuration) {
    utils::check_staffe_init();

    let mut existing_services_config = match read_services_config_json() {
        Ok(c) => c,
        Err(_) => {
            println!("We will create one for you");
            ServicesConfig {
                schema: String::from("1.0.0"),
                services: vec![],
            }
        }
    };

    let mut ids = vec![];
    let mut existing_services_names = vec![];
    for s in existing_services_config.services.iter() {
        ids.push(s.id);
        existing_services_names.push(s.name.clone());
    }
    if existing_services_names.len() != 0 {
        println!(
            "Existing Services in this project are : {} ",
            existing_services_names.join(",").blue()
        );
    }

    match discover_services_api::discover_services_list(&openapi_config).await {
        Ok(services) => {
            let mut services_options = vec![];
            for service in services.iter() {
                if service.id.is_none() {
                    continue;
                }
                if service.id.is_some() && ids.contains(&service.id.unwrap()) {
                    continue;
                }
                services_options.push(Service {
                    id: service.id.unwrap().clone(),
                    name: service.name.clone(),
                    prod_url: service.prod_url.clone(),
                    prod_schema_url: service.prod_schema_url.clone(),
                    stage_url: service.stage_url.clone(),
                    stage_schema_url: service.stage_schema_url.clone(),
                    auth_token_env_key: service.auth_token_env_key.clone(),
                })
            }

            if services_options.len().eq(&0) {
                println!("We could not discover any service to be added. Check with a team lead. It may be possible that you have not added any service to the staffe , you can use {} from the service project ( not this project ) to publish it." , "staffe push".blue());
                exit(1);
            }

            match Select::new(
                "Select a service you want to add to this project",
                services_options,
            )
            .prompt()
            {
                Ok(selected_service) => {
                    existing_services_config.services.push(selected_service);
                    write_services_config_json(existing_services_config);
                    println!(
                        "Now you can use {} to generate / update endpoints from this service",
                        "staffe generate-service-client".blue().underline()
                    )
                }
                Err(_) => {
                    println!("Operation cancelled by the user");
                    exit(1);
                }
            };
        }
        Err(_) => {
            eprintln!("Unable to discover services");
            exit(1)
        }
    };
    println!("Generating service")
}

fn open_api_client_generator(service: &Service) {
    let staffe_config = match utils::read_toml() {
        Ok(c) => c,
        Err(_) => {
            println!("Unable to read the staffe.toml file");
            exit(1);
        }
    };

    let rust_kinds = vec![STAFFE_PACKAGE_KIND::RustBin.to_string()];
    let ts_kinds = vec![
        STAFFE_PACKAGE_KIND::FrontendNext.to_string(),
        STAFFE_PACKAGE_KIND::BackendKoa.to_string(),
        STAFFE_PACKAGE_KIND::TSLib.to_string(),
    ];

    let kind = staffe_config.package.kind.unwrap();

    if rust_kinds.contains(&kind) {
        match Command::new("openapi-generator-cli")
            .arg("generate")
            .arg("-g rust")
            .arg(format!(
                "-i {}{}",
                service.stage_url, service.stage_schema_url
            ))
            .arg(format!("-o {}_client", service.name))
            .arg(format!(
                "--additional-properties=useSingleRequestParameter=true,packageName={}_client",
                service.name
            ))
            .output()
        {
            Ok(cmd_output) => {
                for line in String::from_utf8(cmd_output.stdout)
                    .unwrap()
                    .split("\n")
                    .into_iter()
                {
                    println!("{}", line)
                }
                match Command::new("cargo")
                    .arg("add")
                    .arg(format!("{}_client", service.name))
                    .arg("--path")
                    .arg(format!("{}_client", service.name))
                    .output()
                {
                    Ok(_) => {
                        println!("Added successfully");
                    }
                    Err(e) => {
                        println!("Error occured! {:?}", e);
                        println!("Potentially you dont have cargo install")
                    }
                }
            }
            Err(_) => {}
        }
    } else if ts_kinds.contains(&kind) {
        let mut root_dir = String::from("services");
        if kind.eq(&STAFFE_PACKAGE_KIND::BackendKoa.to_string()) {
            root_dir = String::from("app/services");
        } else if kind.eq(&STAFFE_PACKAGE_KIND::TSLib.to_string()) {
            root_dir = String::from("src/services")
        }
        println!("Generating for {} in {}", kind, root_dir);

        match Command::new("openapi-generator-cli")
            .arg("generate")
            .arg("--generator-name")
            .arg("typescript-fetch")
            .arg(format!(
                "-i {}{}",
                service.stage_url, service.stage_schema_url
            ))
            .arg(format!("-o {}/{}", root_dir, service.name))
            .arg("--additional-properties=typescriptThreePlus=true")
            .output()
        {
            Ok(cmd_output) => {
                for line in String::from_utf8(cmd_output.stdout)
                    .unwrap()
                    .split("\n")
                    .into_iter()
                {
                    println!("{}", line)
                }
            }
            Err(_) => exit(1),
        }
    }

    exit(1);
}

fn present_service_choices_for_client_genration(services_config: ServicesConfig) {
    match Select::new(
        "Please select the service for which you want to generate the client",
        services_config.services,
    )
    .prompt()
    {
        Ok(selected_service) => {
            open_api_client_generator(&selected_service);
        }
        Err(_) => {
            println!("You cancelled the operation");
        }
    }
}

fn print_openapi_generator_not_found() {
    println!("The open API generator is not installed in your machine. Please use {} on MacOS / Windows / Linux" , "npm install @openapitools/openapi-generator-cli -g".green());
    exit(1);
}

pub fn generate_client() {
    utils::check_staffe_init();

    match Command::new("java").arg("-version").output() {
        Ok(cmd_output) => {
            if !String::from_utf8(cmd_output.stderr)
                .unwrap()
                .contains("java version")
            {
                println!("Java is not installed on your machine. Please use https://www.java.com/en/download/help/download_options.html to install Java first");
                exit(1);
            } else {
                match Command::new("openapi-generator-cli")
                    .arg("--version")
                    .output()
                {
                    Ok(cmd_output) => {
                        // println!("{:?}", cmd_output);
                        if !String::from_utf8(cmd_output.stderr)
                            .unwrap()
                            .contains("Usage: openapi-generator-cli")
                        {
                            print_openapi_generator_not_found();
                            exit(1);
                        } else {
                        };
                    }
                    Err(_) => {
                        print_openapi_generator_not_found();
                    }
                }
            };
        }
        Err(_) => {}
    }

    let services_config = match read_services_config_json() {
        Ok(c) => c,
        Err(_) => {
            println!(
                "There is no service configuration found. Please use {} to add one. Exiting",
                "staffe add-service".blue()
            );
            exit(1);
        }
    };
    let raw_args: Vec<String> = env::args().collect();

    if raw_args.len().eq(&2) {
        present_service_choices_for_client_genration(services_config.clone());
        exit(1);
    }

    match raw_args.get(2) {
        Some(service_name) => {
            if service_name.eq("all") {
                for service in services_config.services.iter() {
                    open_api_client_generator(service)
                }
            } else {
                for service in services_config.services.iter() {
                    if service.name.eq(service_name) {
                        open_api_client_generator(service);
                    }
                }
                println!(
                    "The denpendent service {} does not exist in services.config.json",
                    service_name.red()
                );
                present_service_choices_for_client_genration(services_config);
            }
        }
        None => {}
    }
}

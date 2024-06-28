use std::{
    path::Path,
    process::{exit, Command},
};

use colored::Colorize;

use crate::{
    utils::{read_config_file, Service, LANG},
    Environment,
};

fn open_api_client_generator(service: &Service, lang: LANG) {
    let output_dir = format!("test/{}_client", service.name);

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
            println!("Error : {:?}", cmd_output);
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
        Err(err) => {
            print!("{:?}", err)
        }
    }

    exit(1);
}

fn print_openapi_generator_not_found() {
    println!("The open API generator is not installed in your machine. Please use {} on MacOS / Windows / Linux" , "npm install @openapitools/openapi-generator-cli -g".green());
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
        Err(err) => {
            print!("{:?}", err)
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

    for service in services_config.services.unwrap().iter() {
        open_api_client_generator(
            &Service {
                schema_url: format!("https://raw.githubusercontent.com/ginger-society/connector-repo/main/{}/{}.json", service, env),
                name: service.to_string(),
            },
            services_config.lang,
        );
    }
}

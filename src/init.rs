use std::{collections::HashMap, path::Path, process::exit};

use ginger_shared_rs::{write_service_config_file, ServiceConfig, LANG};
use inquire::{InquireError, Select, Text};

pub fn initialize(config_path: &Path) {
    let options = LANG::all();

    let ans: Result<LANG, InquireError> =
        Select::new("Please select the language used in this project", options).prompt();

    match ans {
        Ok(lang_selected) => {
            match Text::new("Where is your service clients going to be generated")
                .with_default("src/services")
                .prompt()
            {
                Ok(dir) => {
                    let config = ServiceConfig {
                        lang: lang_selected,
                        dir: Some(dir.clone()),
                        refs_file: None,
                        services: Some(HashMap::new()),
                        spec_url: Some("/openapi.json".to_string()),
                        urls: Some(HashMap::new()),
                        urls_ws: Some(HashMap::new()),
                        override_name: None,
                        service_type: None,
                        portals_refs: Some(HashMap::new()),
                        ws_refs: Some(HashMap::new()),
                        organization_id: "".to_string(),
                        portal_config: None,
                    };
                    match write_service_config_file(config_path, &config) {
                        Ok(_) => println!("Success!"),
                        Err(_) => println!("Unable to create the configuration. Please check if you have permission to create {:?}" , dir)
                    };
                }
                Err(_) => {
                    println!("Unable to gather all the information needed for initialization");
                    exit(1);
                }
            };
        }
        Err(_) => {
            println!("You must select a language to proceed. Exiting!");
            exit(1);
        }
    };

    ()
}

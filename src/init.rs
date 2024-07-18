use std::{collections::HashMap, path::Path, process::exit};

use inquire::{InquireError, Select, Text};

use crate::utils::{write_config_file, Config, LANG};

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
                    let config = Config {
                        lang: lang_selected,
                        dir: dir.clone(),
                        services: Some(HashMap::new()),
                        spec_url: "".to_string(),
                        urls: HashMap::new(),
                    };
                    match write_config_file(config_path, &config) {
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

use std::{fs::OpenOptions, io::Write, process::Command};

use ginger_shared_rs::LANG;

pub fn generate_arbitrary_client(
    swagger_path: &String,
    lang: LANG,
    server_url: &String,
    out_folder: &String,
) {
    let output_dir = format!("{}", out_folder);
    println!(
        "Generating client for language: {}, from: {}, with server URL: {}",
        lang, swagger_path, server_url
    );

    let language = match lang {
        LANG::TS => String::from("typescript-fetch"),
        LANG::Rust => String::from("rust"),
        LANG::Python => String::from("python"),
        _ => lang.to_string(),
    };
 
    let output = Command::new("openapi-generator-cli")
        .arg("generate")
        .arg("-g")
        .arg(language)
        .arg("-o")
        .arg(&output_dir)
        .arg("--additional-properties")
        .arg("useSingleRequestParameter=true")
        .arg("-i")
        .arg(swagger_path)
        .arg("--skip-validate-spec")
        .output();

    match output {
        Ok(cmd_output) => {
            if cmd_output.status.success() {
                for line in String::from_utf8(cmd_output.stdout)
                    .unwrap()
                    .split('\n')
                    .into_iter()
                {
                    println!("{}", line)
                }
                println!("Client generated successfully in directory: {}", output_dir);
                match lang {
                    LANG::Shell => todo!(),
                    LANG::Rust => todo!(),
                    LANG::TS => {
                        let index_ts_content = format!(
                            "/* tslint:disable */\n/* eslint-disable */\n\nimport {{ DefaultApi }} from './apis'\nimport {{ Configuration }} from './runtime'\n\nexport * from './runtime';\nexport * from './apis/index';\nexport * from './models/index';\n\nconst configuration = new Configuration({{\n  basePath: '{}'\n}})\nconst client = new DefaultApi(configuration)\nexport default client\n",
                            server_url
                        );

                        let index_ts_path = format!("{}/index.ts", output_dir);
                        let mut file = match OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .open(&index_ts_path)
                        {
                            Ok(file) => file,
                            Err(e) => {
                                eprintln!("Failed to create index.ts: {:?}", e);
                                return;
                            }
                        };

                        if let Err(e) = file.write_all(index_ts_content.as_bytes()) {
                            eprintln!("Failed to write to index.ts: {:?}", e);
                        } else {
                            println!("index.ts created successfully at: {}", index_ts_path);
                        }
                    }
                    LANG::Python => todo!(),
                }
            } else {
                eprintln!("Error generating client: {:?}", cmd_output);
                for line in String::from_utf8(cmd_output.stderr)
                    .unwrap()
                    .split('\n')
                    .into_iter()
                {
                    eprintln!("{}", line)
                }
            }
        }
        Err(err) => {
            eprintln!("Failed to execute openapi-generator-cli: {:?}", err);
        }
    }
}

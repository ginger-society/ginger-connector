use std::fs;
use std::path::Path;

pub fn update_python_internal_dependency(
    dependency_name: &str,
    new_version: &str,
    organization: &str,
) {
    // Read the requirements.txt file
    let requirements_path = Path::new("requirements.txt");
    let mut requirements_content =
        fs::read_to_string(requirements_path).expect("Failed to read requirements.txt");

    let mut updated = false;

    // Split the file into lines and process each line
    let new_content: Vec<String> = requirements_content
        .lines()
        .map(|line| {
            let trimmed_line = line.trim();

            // Skip empty lines
            if trimmed_line.is_empty() {
                return line.to_string();
            }

            // Split the line on '#' to handle dependencies with comments
            let parts: Vec<&str> = trimmed_line.split('#').collect();
            let mut dependency = parts[0].trim().to_string();

            // Check if the line contains the specific dependency and organization
            if parts.len() > 1 {
                let org_comment = parts[1].trim();

                // If this line corresponds to the organization and dependency, update it
                if org_comment == organization {
                    if let Some((dep_name, _version)) = dependency.split_once("==") {
                        if dep_name == dependency_name {
                            updated = true;
                            return format!(
                                "{}=={} #{}",
                                dependency_name, new_version, organization
                            );
                        }
                    }
                }
            }

            // Return the original line if no changes were made
            line.to_string()
        })
        .collect();

    // Check if we updated the file content
    if updated {
        // Join the new content back into a single string
        let new_requirements_content = new_content.join("\n");

        // Write the updated content back to the requirements.txt file
        fs::write(requirements_path, new_requirements_content)
            .expect("Failed to write updated requirements.txt");

        println!(
            "Updated internal dependency {} to version {} in requirements.txt.",
            dependency_name, new_version
        );
    } else {
        println!(
            "No matching dependency found for {} in organization {}.",
            dependency_name, organization
        );
    }
}

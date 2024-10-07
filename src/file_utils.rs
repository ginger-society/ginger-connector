use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// Function to replace all instances of a string in a file and save it back
///
/// # Arguments
///
/// * `file_path` - The path of the file where replacements should occur.
/// * `from` - The string to be replaced.
/// * `to` - The replacement string.
///
/// # Returns
///
/// * `io::Result<()>` - Returns `Ok(())` on success, or an `io::Error` on failure.
pub fn replace_in_file(file_path: &str, from: &str, to: &str) -> io::Result<()> {
    // Read the file content
    let path = Path::new(file_path);
    let content = fs::read_to_string(path)?;

    // Replace all instances of the `from` string with the `to` string
    let new_content = content.replace(from, to);

    // Open the file for writing (truncate to overwrite)
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;

    // Write the updated content back to the file
    file.write_all(new_content.as_bytes())?;

    Ok(())
}

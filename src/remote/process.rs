use super::{Error, Result};
use std::process::{Command, Stdio};

pub(super) fn capture(command: &mut Command) -> Result<Vec<u8>> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(Error::input_display)?;
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        return Err(Error::input(format!(
            "{} failed with {}; see subprocess diagnostics on stderr",
            command.get_program().to_string_lossy(),
            output.status
        )));
    }
    Ok(output.stdout)
}

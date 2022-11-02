use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::FigError;

pub fn run_command(
    command: &mut Command,
    parent_command: Option<&mut Command>,
    suppress_std: bool,
) -> crate::Result<()> {
    let parent = if let Some(cmd) = parent_command {
        let proc = if suppress_std {
            cmd.stderr(Stdio::null()).stdout(Stdio::null())
        } else {
            cmd
        };
        let proc = proc.spawn()?;

        // TODO instead of static delay read stdout for matching regex?
        println!(
            "Sleeping 5 seconds to give parent process time to startup. This needs to be fixed!"
        );
        thread::sleep(Duration::from_millis(5_000));

        Some(proc)
    } else {
        None
    };

    let command_proc = if suppress_std {
        command.stderr(Stdio::null()).stdout(Stdio::null())
    } else {
        command
    };
    let mut command_proc = command_proc.spawn()?;

    loop {
        let child_result = command_proc.try_wait()?;

        match child_result {
            Some(status) => {
                // cleanup parent
                if let Some(mut parent_proc) = parent {
                    let parent_result = parent_proc.try_wait()?;

                    if parent_result.is_none() {
                        parent_proc.kill()?;
                    }
                }

                return if status.success() {
                    Ok(())
                } else {
                    Err(FigError::ExecError(
                        "child exited unsuccessfully".to_owned(),
                    ))
                };
            }
            _ => thread::sleep(Duration::from_millis(200)),
        }
    }
}

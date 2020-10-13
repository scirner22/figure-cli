use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
// use std::sync::{Arc, atomic};

use crate::FigError;

pub fn run_command(command: &mut Command, parent_command: Option<&mut Command>) -> crate::Result<()> {
    let parent = if let Some(cmd) = parent_command {
        let proc = cmd
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;

        // TODO instead of static delay read stdout for matching regex?
        println!("Sleeping 10 seconds to give parent process time to startup. This needs to be fixed!");
        thread::sleep(Duration::from_millis(10_000));

        Some(proc)
    } else {
        None
    };
    let mut command_proc = command
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()?;

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
                    Err(FigError::ExecError("child exited unsuccessfully".to_owned()))
                }
            }
            _ => thread::sleep(Duration::from_millis(200))
        }
    }
}

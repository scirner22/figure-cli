use std::process::Command;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, atomic};

use crate::FigError;

fn spawn_signal_handler() -> () {
    let running_state = Arc::new(atomic::AtomicBool::new(true));
    let shared_state = running_state.clone();

    ctrlc::set_handler(move || {
        shared_state.store(false, atomic::Ordering::SeqCst);
    }).unwrap();

    while running_state.load(atomic::Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(200));
    }
}

pub fn run_command(command: &mut Command) -> crate::Result<()> {
    // TODO print command
    // println!("running: ./gradlew clean {}", command);
    let mut child = command.spawn()?;

    thread::spawn(move || {
        spawn_signal_handler();
    });

    loop {
        let child_result = child.try_wait()?;

        match child_result {
            Some(status) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(FigError::ExecError("child exited unsuccessfully".to_owned()))
                }
            }
            _ => thread::sleep(Duration::from_secs(1))
        }
    }
}

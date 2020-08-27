use std::fs;

use clap::{App, Arg, SubCommand};
use walkdir::WalkDir;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::sync::Arc;
use std::sync::atomic;

type Result<T> = std::result::Result<T, String>;

const BUILD: &str = "build";
const TEST: &str = "test";
const RUN: &str = "run";
const MIGRATE: &str = "migrate";

fn project_cmd_about(cmd: &str) -> String {
    format!("Central entry point to {} any \"fig aware\" project type. Supported project types (simple gradle, nested gradle).", cmd)
}

enum ProjectType {
    Gradle,
    Invalid,
}

impl ProjectType {
    fn to_str(&self) -> &str {
        match self {
            ProjectType::Gradle => "build.gradle",
            ProjectType::Invalid => "",
        }
    }
}

fn find_projects(file_name: &str) -> Result<Vec<String>> {
    let mut res = Vec::new();

    for entry in WalkDir::new(".").max_depth(2) {
        // TODO fix unwrap
        let path = entry.unwrap().into_path();
        if path.ends_with(file_name) {
            // TODO fix unwrap
            // TODO remove empty match from root
            res.push(path.parent().unwrap().strip_prefix("./").unwrap().to_str().unwrap().trim().to_owned())
        }
    }

    Ok(res)
}

fn spawn_signal_handler() -> () {
    let running_state = Arc::new(atomic::AtomicBool::new(true));
    let shared_state = running_state.clone();

    ctrlc::set_handler(move || {
        shared_state.store(false, atomic::Ordering::SeqCst);
    }).unwrap();

    while running_state.load(atomic::Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(5));
    }
}

fn run_command(command: &mut Command) -> Result<()> {
    // TODO print command
    // println!("running: ./gradlew clean {}", command);
    let mut child = command.spawn().expect("child spawn");

    thread::spawn(move || {
        spawn_signal_handler();
    });

    loop {
        let child_result = child.try_wait().expect("child result wait");

        match child_result {
            Some(status) => {
                if status.success() {
                    return Ok(());
                } else {
                    // TODO fix errors globally
                    //return Err(Error::ExecError);
                    return Err("temp error in command handler".to_owned());
                }
            }
            None => thread::sleep(Duration::from_secs(5)),
        }
    }
}

fn project_type() -> ProjectType {
    if fs::metadata("build.gradle").is_ok() {
        ProjectType::Gradle
    } else {
        ProjectType::Invalid
    }
}

fn project_cmd(project: Option<&str>, cmd: &str) -> Result<()> {
    let project_type = project_type();
    let mut cmd = cmd.to_owned();
    if let Some(project) = project {
        // TODO remove unwrap
        // TODO validation error when this doesn't contain
        let temp = find_projects(project_type.to_str()).expect("find projects failed").contains(&project.to_owned());
        if !temp {
            println!("{} was not found!", temp);
        }
        cmd = format!("{}:{}", project, cmd);
    }

    match project_type {
        // TODO fix gradle base command
        ProjectType::Gradle => run_command(&mut Command::new("./gradlew")
            .args(vec!["clean", &cmd])).expect("in run command"),
        ProjectType::Invalid => return Err("could not detect project type".to_owned()),
    }

    Ok(())
}

fn main() -> Result<()> {
    let app = App::new("fig - Figure development cli tools")
        .version("0.1")
        .author("Stephen C. <scirner@figure.com>")
        .arg(Arg::with_name("project")
            .short("p")
            .long("project")
            .value_name("PROJECT")
            .takes_value(true)
            .help("Name of nested project to apply SUBCOMMAND to.")
        )
        .subcommand(SubCommand::with_name(BUILD)
            .about(project_cmd_about(BUILD).as_str())
        )
        .subcommand(SubCommand::with_name(TEST)
            .about(project_cmd_about(TEST).as_str())
        )
        .subcommand(SubCommand::with_name(RUN)
            .about(project_cmd_about(RUN).as_str())
        )
        .subcommand(SubCommand::with_name(MIGRATE)
            .about(project_cmd_about(MIGRATE).as_str())
        )
        .get_matches();

    let project = app.value_of("project");
    match app.subcommand_name() {
        Some(BUILD) => project_cmd(project, BUILD)?,
        Some(TEST) => project_cmd(project, TEST)?,
        Some(RUN) => project_cmd(project, RUN)?,
        Some(MIGRATE) => project_cmd(project, MIGRATE)?,
        _ => {},
    }

    Ok(())
}

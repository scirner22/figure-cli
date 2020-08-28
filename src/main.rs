#[macro_use]
extern crate quick_error;

use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::sync::Arc;
use std::sync::atomic;
use std::collections::HashMap;

use clap::{App, Arg, SubCommand};
use serde::Deserialize;
use walkdir::WalkDir;

type Result<T> = std::result::Result<T, Error>;

// TODO
// break up into separate files
// read from config file for database fields
// create config file based on other facts - make sure it's added to .gitignore

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        ProjectTypeError(s: String) {}
        ExecError(s: String) {}
        WalkDirError(e: walkdir::Error) {
            // display("{}", err)
            from()
        }
        IoError(e: std::io::Error) {
            from()
        }
        TomlError(e: toml::de::Error) {
            from()
        }
    }
}

const BUILD: &str = "build";
const TEST: &str = "test";
const RUN: &str = "run";
const MIGRATE: &str = "migrate";
const POSTGRES_CLI: &str = "psql";

#[derive(Deserialize, Debug)]
struct Config {
    postgres_local: Option<PostgresConfig>,
    postgres_test: Option<PostgresConfig>,
    postgres_prod: Option<PostgresConfig>,
}

#[derive(Deserialize, Debug)]
struct PostgresConfig {
    host: Option<String>,
    port: Option<u16>,
    user: String,
    password: String,
    database: String,
    schema: Option<String>,
}

fn project_cmd_about(cmd: &str) -> String {
    format!("Central entry point to {} any \"fig aware\" project type. Supported project types (simple gradle, nested gradle).", cmd)
}

enum ProjectType {
    Gradle,
    Invalid,
}

#[derive(Deserialize, Eq, Hash, PartialEq)]
enum EnvironmentType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "prod")]
    Production,
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
        let path = entry?.into_path();
        if path.ends_with(file_name) {
            // TODO remove empty match from root
            res.push(
                path.parent().expect("E01 - no parent directory")
                    .strip_prefix("./").expect("E02 - could not strip prefix")
                    .to_str().expect("E03 - could not convert string")
                    .trim().to_owned()
            )
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
        thread::sleep(Duration::from_millis(200));
    }
}

fn run_command(command: &mut Command) -> Result<()> {
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
                    Err(Error::ExecError("child exited unsuccessfully".to_owned()))
                }
            }
            None => thread::sleep(Duration::from_secs(1)),
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

fn environment_type(env: Option<&str>) -> EnvironmentType {
    match env {
        Some("local") => EnvironmentType::Local,
        Some("test") => EnvironmentType::Test,
        Some("prod") => EnvironmentType::Production,
        _ => EnvironmentType::Local,
    }
}

fn project_cmd(project: Option<&str>, cmd: &str) -> Result<()> {
    let mut cmd = cmd.to_owned();
    if let Some(project) = project {
        cmd = format!("{}:{}", project, cmd);
    }

    match project_type() {
        // TODO fix gradle base command
        ProjectType::Gradle => run_command(&mut Command::new("./gradlew")
            .args(vec!["clean", &cmd])
        ),
        ProjectType::Invalid => Err(Error::ProjectTypeError("could not detect project type".to_owned())),
    }
}

fn postgres_cli_cmd(env: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("psql");

    match environment_type(env) {
        EnvironmentType::Local => {
            cmd.env("PGPASSWORD", "password1");
            cmd.env("PGOPTIONS", "--search_path=p8e");
            cmd.args(vec!["-h", "localhost", "-U", "postgres", "p8e"]);
        },
        // TOOD implement other env types :) this is reachable
        _ => unreachable!(),
    };

    run_command(&mut cmd)
}

fn main() -> Result<()> {
    let toml_string = fs::read_to_string(".fig.toml")?;
    let config: Config = toml::from_str(&toml_string)?;
    println!("{:?}", config);

    let project_arg = Arg::with_name("project")
        .short("p")
        .long("project")
        .value_name("PROJECT")
        .takes_value(true)
        .help("Name of nested project to apply SUBCOMMAND to.");
    let env_arg = Arg::with_name("environment")
        .short("e")
        .long("environment")
        .value_name("ENV")
        .takes_value(true)
        .possible_values(&["local", "test", "prod"])
        .help("Environment to apply SUBCOMMAND to.");

    let app = App::new("fig - Figure development cli tools")
        .version("0.1")
        .author("Stephen C. <scirner@figure.com>")
        .subcommand(SubCommand::with_name(BUILD)
            .arg(&project_arg)
            .about(project_cmd_about(BUILD).as_str())
        )
        .subcommand(SubCommand::with_name(TEST)
            .arg(&project_arg)
            .about(project_cmd_about(TEST).as_str())
        )
        .subcommand(SubCommand::with_name(RUN)
            .arg(&project_arg)
            .about(project_cmd_about(RUN).as_str())
        )
        .subcommand(SubCommand::with_name(MIGRATE)
            .arg(&project_arg)
            .about(project_cmd_about(MIGRATE).as_str())
        )
        .subcommand(SubCommand::with_name(POSTGRES_CLI)
            .arg(&env_arg)
            .about(project_cmd_about(MIGRATE).as_str())
        )
        .get_matches();

    let project = app.value_of("project");
    let environment = app.value_of("environment");

    match app.subcommand_name() {
        Some(BUILD) => project_cmd(project, BUILD)?,
        Some(TEST) => project_cmd(project, TEST)?,
        Some(RUN) => project_cmd(project, RUN)?,
        Some(MIGRATE) => project_cmd(project, MIGRATE)?,

        Some(POSTGRES_CLI) => postgres_cli_cmd(environment)?,
        _ => {},
    }

    Ok(())
}

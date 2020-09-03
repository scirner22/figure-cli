#[macro_use]
extern crate quick_error;

use clap::{App, Arg, SubCommand};
use std::fs;
use std::process::Command;
use walkdir::WalkDir;

use consts::*;
use config::{EnvironmentType, environment_type, get_config};

mod config;
mod consts;
mod runner;

// TODO
// break up into separate files
// read from config file for database fields
// create config file based on other facts - make sure it's added to .gitignore

pub type Result<T> = std::result::Result<T, FigError>;

quick_error! {
    #[derive(Debug)]
    pub enum FigError {
        ExecError(s: String) {}
        ProjectTypeError(s: String) {}
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

fn project_type() -> ProjectType {
    if fs::metadata("build.gradle").is_ok() {
        ProjectType::Gradle
    } else {
        ProjectType::Invalid
    }
}


fn project_cmd(project: Option<&str>, cmd: &str) -> Result<()> {
    let mut cmd = cmd.to_owned();
    if let Some(project) = project {
        cmd = format!("{}:{}", project, cmd);
    }

    match project_type() {
        // TODO fix gradle base command
        ProjectType::Gradle => runner::run_command(&mut Command::new("./gradlew")
            .args(vec!["clean", &cmd])
        ),
        ProjectType::Invalid => Err(FigError::ProjectTypeError("could not detect project type".to_owned())),
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

    runner::run_command(&mut cmd)
}

fn project_cmd_about(cmd: &str) -> String {
    format!("Central entry point to {} any \"fig aware\" project type. Supported project types (simple gradle, nested gradle).", cmd)
}

fn main() -> Result<()> {
    let config = get_config(".fig.toml")?;
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

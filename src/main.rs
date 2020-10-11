#[macro_use]
extern crate quick_error;

use clap::{App, Arg, SubCommand};
use std::fs;
use std::process::Command;

use consts::*;
use config::{EnvironmentType, environment_type, get_config};
use crate::config::{Config, PostgresConfig, PostgresConfigType};
use std::path::Path;
use crate::FigError::ConfigError;

mod config;
mod consts;
mod runner;
mod util;

pub type Result<T> = std::result::Result<T, FigError>;

quick_error! {
    #[derive(Debug)]
    pub enum FigError {
        ConfigError(s: String) {}
        ExecError(s: String) {}
        EnvError(s: String) {}
        GitIgnore(s: gitignore::Error) {
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

fn postgres_shell_cmd(postgres_config: &PostgresConfig, port: u16) -> Command {
    let mut cmd = Command::new("psql");

    let port = match &postgres_config._type {
        PostgresConfigType::Kubernetes { .. } => port,
        PostgresConfigType::GCloudProxy { .. } => port,
        PostgresConfigType::Direct => postgres_config.port(),
    };

    cmd.env("PGPASSWORD", &postgres_config.password);
    cmd.env("PGOPTIONS", format!("--search_path={}", &postgres_config.schema()));
    cmd.args(
        vec![
            "-h", &postgres_config.host(),
            "-U", &postgres_config.user,
            "-p", &port.to_string(),
            &postgres_config.database,
        ]
    );

    return cmd
}

fn postgres_tunnel_cmd(postgres_config: &PostgresConfig, port: u16) -> Result<Option<Command>> {
    match &postgres_config._type {
        PostgresConfigType::Kubernetes { context, namespace, deployment } => {
            let mut cmd = Command::new("kubectl");
            cmd.args(
                vec![
                    "--context", context,
                    "--namespace", namespace,
                    "port-forward",
                    &format!("deployment/{}", deployment),
                    &format!("{}:{}", port, &postgres_config.port()),
                ]
            );

            Ok(Some(cmd))
        }
        PostgresConfigType::GCloudProxy { instance } => {
            let mut cmd = Command::new("cloud_sql_proxy");
            cmd.args(vec!["-instances", &format!("{}=tcp:{}", instance, port)]);

            Ok(Some(cmd))
        }
        PostgresConfigType::Direct => {
                Ok(None)
            }
        }
}

fn postgres_cli_cmd(config: &Config, env: Option<&str>) -> Result<()> {
    let port = util::find_available_port()?;
    println!("found open port {}", port);

    let config = match environment_type(env)? {
        EnvironmentType::Local => {
            config.postgres_local.as_ref().ok_or(FigError::ConfigError("[postgres_local] block is invalid".to_owned()))?
        },
        EnvironmentType::Test => {
            config.postgres_test.as_ref().ok_or(FigError::ConfigError("[postgres_test] block is invalid".to_owned()))?
        },
        EnvironmentType::Production => {
            config.postgres_prod.as_ref().ok_or(FigError::ConfigError("[postgres_prod] block is invalid".to_owned()))?
        },
    };

    runner::run_command(
        &mut postgres_shell_cmd(config, port),
        postgres_tunnel_cmd(config, port)?.as_mut(),
    )
}

fn project_cmd_about() -> String {
    format!("Opens a postgres shell on a randomly available port.")
}

fn check_gitignore(config_path: &Path) -> Result<()> {
    let gitignore_path = fs::canonicalize(Path::new(".gitignore"))?;
    let config_path = fs::canonicalize(config_path)?;
    let file = gitignore::File::new(gitignore_path.as_path())?;

    if !file.is_excluded(config_path.as_path())? {
        Err(ConfigError(".fig.toml must be excluded in your .gitignore".to_owned()))
    } else {
        Ok(())
    }
}

fn main() -> Result<()> {
    // TODO on this error make sure printed messages shows you how to create a config file
    let config_path = Path::new(".fig.toml");
    let config = get_config(config_path)?;

    // check gitignore contains exclusion for fig config since it contains secrets
    // TODO bypass this for non action commands
    check_gitignore(config_path)?;

    let env_arg = Arg::with_name("environment")
        .required(true)
        .short("e")
        .long("environment")
        .value_name("ENV")
        .takes_value(true)
        .possible_values(&["local", "test", "prod"])
        .help("Environment to apply SUBCOMMAND to.");

    let app = App::new("fig - Figure development cli tools")
        .version("0.1.0")
        .author("Stephen C. <scirner@figure.com>")
        .subcommand(SubCommand::with_name(POSTGRES_CLI)
            .arg(&env_arg)
            .about(project_cmd_about().as_str())
        )
        .get_matches();

    match app.subcommand_name() {
        Some(POSTGRES_CLI) => postgres_cli_cmd(
            &config,
            app.subcommand_matches(POSTGRES_CLI).unwrap().value_of("environment"),
        )?,
        _ => {},
    }

    Ok(())
}

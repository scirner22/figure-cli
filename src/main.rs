#[macro_use]
extern crate quick_error;

use clap::{App, Arg, SubCommand};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use consts::*;
use config::{EnvironmentType, environment_type, get_config};
use crate::config::{Config, PostgresConfig, PostgresConfigType};
use crate::runner::run_command;

mod config;
mod consts;
mod runner;
mod util;

pub type Result<T> = std::result::Result<T, FigError>;

quick_error! {
    #[derive(Debug)]
    pub enum FigError {
        ConfigError(s: String) {}
        DoctorError(s: String) {}
        ExecError(s: String) {}
        EnvError(s: String) {}
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
        false,
    )
}

fn doctor_cmd(cmd: &str, args: Vec<&str>) -> Result<()> {
    let mut runnable = Command::new(cmd);
    runnable.args(args);

    run_command(&mut runnable, None, true)
        .map(|_| println!("[*] {} is installed", cmd))
        .map_err(|e| { println!("[ ] {} is not installed", cmd); e })
}

fn init_cmd<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.as_ref())?;

    println!("Writing config file to {}", path.as_ref().to_str().unwrap());

    file.write_all(
        br##"# fig-cli configuration

[postgres_local]
type = "direct"
user = "postgres"
password = "password1"
database = "object_store"
schema = "object_store"

[postgres_test]
type = { kubernetes = { context = "gke_figure-development_us-east1-b_tf-test", namespace = "p8e", deployment = "p8e-api-db-deployment" } }
user = "p8e-api"
password = "password1"
database = "p8e-api"
schema = "p8e-api"

[postgres_prod]
type = { gcloudproxy = { instance = "figure-production:us-east1:service-identity-db" } }
user = "<insert user name>"
password = "<insert password>"
database = "service-identity-db"
schema = "service_identity"
"##)?;

    Ok(())
}

fn main() -> Result<()> {
    let mut default_config_path = dirs::config_dir().unwrap();
    default_config_path.push(FIG_CONFIG_DIR);
    if !std::path::Path::new(&default_config_path).exists() {
        std::fs::create_dir(&default_config_path)?;
    }

    default_config_path.push(std::env::current_dir()?.file_name().unwrap());
    if !std::path::Path::new(&default_config_path).exists() {
        std::fs::create_dir(&default_config_path)?;
    }

    let env_arg = Arg::with_name("environment")
        .required(true)
        .short("e")
        .long("environment")
        .value_name("ENV")
        .takes_value(true)
        .possible_values(&["local", "test", "prod"])
        .help("Environment to apply SUBCOMMAND to.");
    let config_arg = Arg::with_name("config")
        .required(false)
        .short("c")
        .long("config")
        .value_name("CONF")
        .takes_value(true)
        .default_value("default")
        .help("Config name to read toml configuration from.");

    let app = App::new("fig - Figure development cli tools")
        .version("0.3.0")
        .author("Stephen C. <scirner@figure.com>")
        .arg(config_arg)
        .subcommand(SubCommand::with_name(DOCTOR)
            .about(format!("Checks if all required dependencies are installed").as_ref())
        )
        .subcommand(SubCommand::with_name(INIT)
            .about(format!("Installs a stub configuration file with examples to help with setup").as_ref())
        )
        .subcommand(SubCommand::with_name(POSTGRES_CLI)
            .arg(&env_arg)
            .about(format!("Opens a postgres shell on a randomly available port").as_ref())
        )
        .get_matches();

    let mut config_path = default_config_path;
    config_path.push(app.value_of("config").unwrap());
    config_path.set_extension("toml");

    match app.subcommand_name() {
        Some(DOCTOR) => {

            let commands = vec![
                doctor_cmd("kubectl", vec!["version"]),
                doctor_cmd("psql", vec!["--version"]),
                doctor_cmd("gcloud", vec!["version"]),
            ];

            if commands.iter().any(|res| res.is_err()) {
                return Err(FigError::DoctorError("Please make sure all of the above checks are successful!".to_owned()))
            }
        },
        Some(INIT) => {
            init_cmd(config_path)?;
        },
        Some(POSTGRES_CLI) => {
            // TODO on this error make sure printed messages shows you how to create a config file
            let config = get_config(config_path)?;

            postgres_cli_cmd(
                &config,
                app.subcommand_matches(POSTGRES_CLI).unwrap().value_of("environment"),
            )?
        },
        _ => {},
    }

    Ok(())
}

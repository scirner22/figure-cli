#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate prettytable;

use clap::{App, Arg, SubCommand, value_t};
use std::{fs::OpenOptions, fs::File};
use std::env::temp_dir;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use consts::*;
use config::{EnvironmentType, environment_type, get_config};
use crate::config::{Config, PostgresConfig, PostgresConfigType};
use prettytable::{Table, format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR};
use crate::runner::run_command;
use uuid::Uuid;

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
        ParseError(s: String) {}
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

fn temp_file(extension: &str) -> PathBuf {
    let mut dir = temp_dir();
    let file_name = format!("{}.{}", Uuid::new_v4(), extension);

    dir.push(file_name);

    dir
}

fn postgres_pgbouncer_cmd(postgres_config: &PostgresConfig, port: u16, upstream_port: u16) -> Result<Command> {
    let userlist_file_path_str = temp_file("txt");
    let mut userlist_file = File::create(userlist_file_path_str.clone())?;
    let ini_file_path_str = temp_file("ini");
    let mut ini_file = File::create(ini_file_path_str.clone())?;

    userlist_file.write_all(format!("\"{}\" \"{}\"", &postgres_config.user, &postgres_config.password).as_bytes())?;

    // TODO change to md5 hash of password
    // TODO remove
    println!("{}", ini_file_path_str.to_str().unwrap());
    println!("\"{}\" \"{}\"", &postgres_config.user, &postgres_config.password);

    let ini_content = format!(
        r#"################## fig-cli pgbouncer configuration ##################

[databases]
{} = host=localhost port={} user={} dbname={} password={}

[pgbouncer]
listen_addr = 0.0.0.0
listen_port = {}
unix_socket_dir =
auth_type = any
pool_mode = transaction
default_pool_size = 1
ignore_startup_parameters = extra_float_digits

################## end file ##################
"#,
        postgres_config.database,
        upstream_port,
        postgres_config.user,
        postgres_config.database,
        postgres_config.password,
        port,
    );

    ini_file.write_all(ini_content.as_bytes())?;

    let mut cmd = Command::new("pgbouncer");

    cmd.args(vec![ini_file_path_str.to_str().unwrap()]);

    let mut table = Table::new();

    table.set_format(*FORMAT_NO_BORDER_LINE_SEPARATOR);
    table.set_titles(row!["KEY", "VALUE"]);

    table.add_row(row![
        "connection string",
        format!("postgresql://localhost:{}/{}", port, postgres_config.database)]);
    table.add_row(row!["host", "localhost"]);
    table.add_row(row!["port", port.to_string()]);
    table.add_row(row!["database", postgres_config.database]);

    table.printstd();

    Ok(cmd)
}

fn postgres_cli_cmd(config: &Config, env: Option<&str>, port: Option<u16>, interative_shell: bool) -> Result<()> {
    let port = match port {
        Some(port) => port,
        None => {
            let port = util::find_available_port()?;
            println!("Found random open port {}", port);
            port
        },
    };

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

    if interative_shell {
        runner::run_command(
            &mut postgres_shell_cmd(config, port),
            postgres_tunnel_cmd(config, port)?.as_mut(),
            false,
        )
    } else {
        let bridge_port = util::find_available_port()?;
        println!("Using random open port for bridge {}", bridge_port);

        runner::run_command(
            &mut postgres_pgbouncer_cmd(config, port, bridge_port)?,
            postgres_tunnel_cmd(config, bridge_port)?.as_mut(),
            false,
        )
    }
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

    let static_port_arg = Arg::with_name("port")
        .short("p")
        .long("port")
        .value_name("PORT")
        .takes_value(true)
        .help("Optional static port. If omitted, a random open port is chosen.");
    let interactive_shell_args = Arg::with_name("shell")
        .short("s")
        .long("shell")
        .value_name("SHELL")
        .takes_value(false)
        .help("Optionally starts a psql shell.");
    let env_arg = Arg::with_name("environment")
        .required(true)
        .index(1)
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
        .version("0.5.1")
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
            .arg(&static_port_arg)
            .arg(&interactive_shell_args)
            .about(format!("Proxies a remote postgres connection").as_ref())
        )
        .get_matches();

    let mut config_path = default_config_path;
    config_path.push(app.value_of("config").unwrap());
    config_path.set_extension("toml");

    match app.subcommand_name() {
        Some(DOCTOR) => {
            let commands = vec![
                doctor_cmd("kubectl", vec![""]),
                doctor_cmd("psql", vec!["--version"]),
                doctor_cmd("gcloud", vec!["version"]),
                doctor_cmd("pgbouncer", vec!["--version"]),
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
            let values = app.subcommand_matches(POSTGRES_CLI).unwrap();
            let port = match value_t!(values.value_of("port"), u16) {
                Ok(port) => Ok(Some(port)),
                Err(e) => match e.kind {
                    clap::ErrorKind::ArgumentNotFound => Ok(None),
                    clap::ErrorKind::InvalidValue => Err(FigError::ParseError("Could not parse port to u16.".to_owned())),
                    // TDOO figure out which error conditions we need to add
                    _ => unreachable!()
                },
            }?;
            let interactive_shell = values.is_present("shell");

            postgres_cli_cmd(&config, values.value_of("environment"), port, interactive_shell)?
        },
        _ => {},
    }

    Ok(())
}

#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate prettytable;

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf, StripPrefixError};
use std::process::Command;
use std::{env, fs};

use crate::config::{Config, PortForwardConfig, PostgresConfig, ServerConfigType};
use crate::runner::run_command;
use crate::util::ForwardingInfo;
use clap::{value_t, App, Arg, SubCommand};
use config::{environment_type, get_config, EnvironmentType};
use consts::*;
use prettytable::{format::consts::FORMAT_NO_BORDER_LINE_SEPARATOR, Table};
use uuid::Uuid;
use walkdir::WalkDir;

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
        StripPrefixError(e: StripPrefixError) {
            from()
        }
        TomlError(e: toml::de::Error) {
            from()
        }
        WalkdirError(e: walkdir::Error) {
            from()
        }
        UuidError(e: uuid::Error) {
            from()
        }
    }
}

/// Recursively walks `path`, collecting any files, optionally filtering by
/// suffix
fn collect_files<P: AsRef<Path>>(path: P, match_suffix: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut paths = WalkDir::new(path.as_ref())
        .into_iter()
        .map(|e| e.map(|p| p.path().to_path_buf()))
        .collect::<std::result::Result<Vec<PathBuf>, walkdir::Error>>()
        .map_err(Into::<FigError>::into)?;
    if let Some(suffix) = match_suffix {
        paths = paths
            .into_iter()
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(suffix))
            .collect::<Vec<_>>();
    }
    paths.sort();
    Ok(paths)
}

fn generate_kong_api_keys(namespace: &str, name: &str, acl_group: &str, uuid: &Uuid) -> Result<()> {
    let kong_resources = format!(
        "---
apiVersion: configuration.konghq.com/v1
kind: KongConsumer
metadata:
  annotations:
    konghq.com/plugins: {name}-request-transformer
    kubernetes.io/ingress.class: kong
  name: {name}
  namespace: {namespace}
username: {name}
credentials:
- {name}-kong-keyauth
- {name}-kong-acl
---
apiVersion: configuration.konghq.com/v1
config:
  remove:
    headers:
    - x-uuid
    querystring:
    - apikey
  add:
    headers:
    - x-uuid:{uuid}
consumerRef: {name}
kind: KongPlugin
metadata:
  name: {name}-request-transformer
  namespace: {namespace}
plugin: request-transformer"
    );

    let acl_group = base64::encode(acl_group);
    let password = util::random_alphanum(50);
    let password = base64::encode(&password);

    let secret_resources = format!(
        "---
apiVersion: v1
data:
  group: {acl_group}
  kongCredType: YWNs
kind: Secret
metadata:
  name: {name}-kong-acl
  namespace: {namespace}
type: Opaque
---
apiVersion: v1
data:
  key: {password}
  kongCredType: a2V5LWF1dGg=
kind: Secret
metadata:
  name: {name}-kong-keyauth
  namespace: {namespace}
type: Opaque"
    );

    println!("{}", kong_resources);
    eprintln!("{}", secret_resources);

    Ok(())
}

fn k8s_port_forward(
    config: Option<&PortForwardConfig>,
    forwarding: &ForwardingInfo,
    context: Option<&str>,
    namespace: Option<&str>,
) -> Result<()> {
    let (config_context, config_namespace) = match config {
        Some(config) => (Some(config.context.as_str()), config.namespace.as_deref()),
        None => (None, None),
    };

    // override the values from the config if `context` and `namespace` are explicitly provided:
    let context_arg = context
        .or(config_context)
        .map_or_else(|| "".to_owned(), |c| format!("--context={}", c));
    let namespace_arg = namespace
        .or(config_namespace)
        .map_or_else(|| "".to_owned(), |n| format!("--namespace={}", n));
    let pod_name = format!("figcli-temp-port-forward-{}", util::random_alphanum(8));

    let source_contents = format!(
        include_str!("../template/kubectl-port-forward-remote-host.sh.template"),
        temp_pod_name = pod_name,
        context_arg = context_arg,
        namespace_arg = namespace_arg,
        local_port = forwarding.local_port,
        remote_host = forwarding.remote_host,
        remote_port = forwarding.remote_port
    );

    // Write the parameterized template out as a shell script to execute:
    let shell_script_name = util::temp_file("sh");
    let mut shell_script_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&shell_script_name)?;

    shell_script_file.write_all(source_contents.as_bytes())?;

    let mut port_forward_script = Command::new(shell_script_name.to_str().unwrap());

    println!(
        "Forwarding {}:{} -> {}",
        forwarding.remote_host, forwarding.remote_port, forwarding.local_port
    );

    runner::run_command(&mut port_forward_script, None, false)
}

fn postgres_shell_cmd(config: &PostgresConfig, port: u16) -> Command {
    let mut cmd = Command::new("psql");

    let port = match &config._type {
        ServerConfigType::Kubernetes { .. } => port,
        ServerConfigType::GCloudProxy { .. } => port,
        ServerConfigType::Direct => config.port(),
    };

    if let Some(password) = &config.password {
        cmd.env("PGPASSWORD", password);
    }

    cmd.env("PGOPTIONS", format!("--search_path={}", &config.schema()));
    cmd.args(vec![
        "-h",
        &config.host(),
        "-U",
        &config.user,
        "-p",
        &port.to_string(),
        &config.database,
    ]);

    cmd
}

fn postgres_tunnel_cmd(config: &PostgresConfig, port: u16) -> Result<Option<Command>> {
    match &config._type {
        ServerConfigType::Kubernetes {
            context,
            namespace,
            deployment,
            container,
        } => {
            let mut cmd = Command::new("kubectl");
            let deployment_spec = format!("deployment/{}", deployment);
            let port_mapping_spec = format!("{}:{}", port, &config.port());

            let mut parts: Vec<&str> = vec!["--context", context, "--namespace", namespace];
            if let Some(container) = container {
                parts.extend(&vec!["-c", container]);
            }
            parts.extend(&vec!["port-forward", &deployment_spec, &port_mapping_spec]);

            cmd.args(parts);

            Ok(Some(cmd))
        }
        ServerConfigType::GCloudProxy { instance } => {
            let mut cmd = Command::new("cloud_sql_proxy");
            cmd.args(vec!["-instances", &format!("{}=tcp:{}", instance, port)]);

            Ok(Some(cmd))
        }
        ServerConfigType::Direct => Ok(None),
    }
}

fn postgres_pgbouncer_cmd(
    config: &PostgresConfig,
    port: u16,
    upstream_port: u16,
) -> Result<Command> {
    let ini_file_path_str = util::temp_file("ini");
    let mut ini_file = fs::File::create(&ini_file_path_str)?;

    let password = config.password.as_ref().ok_or_else(|| {
        FigError::ConfigError("password required when using pgbouncer".to_owned())
    })?;

    // TODO change to md5 hash of password
    // TODO remove
    println!("{}", ini_file_path_str.display());
    println!("\"{}\" \"{}\"", &config.user, &password);

    let ini_content = format!(
        include_str!("../template/pgbouncer.toml.template"),
        database = config.database,
        upstream_port = upstream_port,
        user = config.user,
        password = password,
        listen_port = port
    );

    ini_file.write_all(ini_content.as_bytes())?;

    let mut cmd = Command::new("pgbouncer");

    cmd.args(vec![ini_file_path_str.to_str().unwrap()]);

    let mut table = Table::new();

    table.set_format(*FORMAT_NO_BORDER_LINE_SEPARATOR);
    table.set_titles(row!["KEY", "VALUE"]);

    table.add_row(row![
        "connection string",
        format!("postgresql://localhost:{}/{}", port, config.database)
    ]);
    table.add_row(row!["host", "localhost"]);
    table.add_row(row!["port", port.to_string()]);
    table.add_row(row!["database", config.database]);

    table.printstd();

    Ok(cmd)
}

fn postgres_cli_cmd(
    config: &Config,
    env: Option<&str>,
    port: Option<u16>,
    interative_shell: bool,
    use_pgbouncer: bool,
) -> Result<()> {
    let port = match port {
        Some(port) => port,
        None => {
            let port = util::find_available_port()?;
            println!("Found random open port {}", port);
            port
        }
    };

    let postgres_config =
        match environment_type(env)? {
            EnvironmentType::Local => config.postgres_local.as_ref().ok_or_else(|| {
                FigError::ConfigError("[postgres_local] block is invalid".to_owned())
            })?,
            EnvironmentType::Test => config.postgres_test.as_ref().ok_or_else(|| {
                FigError::ConfigError("[postgres_test] block is invalid".to_owned())
            })?,
            EnvironmentType::Production => config.postgres_prod.as_ref().ok_or_else(|| {
                FigError::ConfigError("[postgres_prod] block is invalid".to_owned())
            })?,
        };

    if interative_shell {
        runner::run_command(
            &mut postgres_shell_cmd(postgres_config, port),
            postgres_tunnel_cmd(postgres_config, port)?.as_mut(),
            false,
        )
    } else if use_pgbouncer {
        let bridge_port = util::find_available_port()?;
        println!(
            "Using pgbouncer with random open port for bridge {}",
            bridge_port
        );
        runner::run_command(
            &mut postgres_pgbouncer_cmd(postgres_config, port, bridge_port)?,
            postgres_tunnel_cmd(postgres_config, bridge_port)?.as_mut(),
            false,
        )
    } else {
        println!("Using default port forwarding");
        let mut forward_command = postgres_tunnel_cmd(postgres_config, port)?
            .expect("couldn't build port-forward command");
        runner::run_command(&mut forward_command, None, false)
    }
}

fn doctor_cmd(cmd: &str, args: Vec<&str>) -> Result<()> {
    let mut runnable = Command::new(cmd);
    runnable.args(args);

    run_command(&mut runnable, None, true)
        .map(|_| println!("[*] {} is installed", cmd))
        .map_err(|e| {
            println!("[ ] {} is not installed", cmd);
            e
        })
}

fn config_init_cmd<P: AsRef<Path>>(path: P, force: bool, from: Option<(P, P)>) -> Result<()> {
    let write_file = if force {
        true
    } else {
        util::prompt_on_write(&path)
    };

    if !write_file {
        return Ok(());
    }

    // If a path is supplied, copy an existing configuration file to create a
    // new configuration.
    // - `config_file_base_path` is base location of configuration files
    // - `app_config_path` the path, as it appears in `config list -A`, e.g.
    //   "figure-cli/provenance.toml"
    // - `path` is the destination path of the configuration file to be written
    if let Some((app_config_path, config_file_base_path)) = from {
        let target_config_file: Option<(PathBuf, PathBuf)> =
            collect_files(&config_file_base_path, Some("toml"))?
                .into_iter()
                .flat_map(|p| {
                    p.strip_prefix(config_file_base_path.as_ref())
                        .map(|p_prefix| (p.clone(), p_prefix.to_path_buf()))
                })
                .find(|(_, prefix_path)| prefix_path == app_config_path.as_ref());

        return match target_config_file {
            Some((app_config_full_path, _)) => {
                let result = fs::copy(app_config_full_path, &path)
                    .map(|_| ())
                    .map_err(Into::into);
                println!("Writing config file to {}", path.as_ref().display());
                result
            }
            None => Err(FigError::ConfigError(format!(
                "Can't copy configuration {}",
                app_config_path.as_ref().display()
            ))),
        };
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.as_ref())?;

    println!("Writing config file to {}", path.as_ref().display());
    file.write_all(include_bytes!("../template/config.toml.example"))?;

    Ok(())
}

fn config_show_contents<P: AsRef<Path>>(path: P) -> Result<()> {
    let contents = fs::read_to_string(path.as_ref())?;
    println!("{}", contents);
    Ok(())
}

fn config_show_path<P: AsRef<Path>>(path: P, check: bool) -> Result<()> {
    let path = path.as_ref();
    if check {
        let exists = path.exists();
        println!(
            "Checking - {} {}",
            path.display(),
            if exists { GREEN_CHECK_ICON } else { RED_X_ICON }
        );
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

fn config_edit_path<P: AsRef<Path>>(path: P) -> Result<()> {
    let file_path = path.as_ref().to_str().unwrap();
    // use whatever the system envvar "EDITOR" is set to:
    let system_editor = env::var("EDITOR").unwrap_or_else(|_| DEFAULT_EDITOR.to_owned());
    let mut editor_cmd = Command::new(system_editor);
    editor_cmd.arg(file_path);
    runner::run_command(&mut editor_cmd, None, false)?;
    Ok(())
}

fn config_list_files<P: AsRef<Path>>(path: P) -> Result<()> {
    for p in collect_files(&path, Some("toml"))? {
        println!("{}", p.strip_prefix(&path)?.display());
    }
    Ok(())
}

fn get_config_paths() -> Result<(PathBuf, PathBuf)> {
    let mut default_config_path = dirs::config_dir().unwrap();
    default_config_path.push(FIG_CONFIG_DIR);
    let base_config_path = default_config_path.clone();

    if !std::path::Path::new(&default_config_path).exists() {
        std::fs::create_dir(&default_config_path)?;
    }

    default_config_path.push(std::env::current_dir()?.file_name().unwrap());
    if !std::path::Path::new(&default_config_path).exists() {
        std::fs::create_dir(&default_config_path)?;
    }

    Ok((default_config_path, base_config_path))
}

fn main() -> Result<()> {
    let (default_config_path, base_config_path) = {
        let mut default_config_path = dirs::config_dir().unwrap();
        default_config_path.push(FIG_CONFIG_DIR);
        let base_config_path = default_config_path.clone();

        if !std::path::Path::new(&default_config_path).exists() {
            std::fs::create_dir(&default_config_path)?;
        }

        default_config_path.push(std::env::current_dir()?.file_name().unwrap());
        if !std::path::Path::new(&default_config_path).exists() {
            std::fs::create_dir(&default_config_path)?;
        }

        (default_config_path, base_config_path)
    };
    let rand_uuid = Uuid::new_v4().hyphenated().to_string();
    let static_port_arg = Arg::with_name("port")
        .short("p")
        .long("port")
        .value_name("PORT")
        .takes_value(true)
        .help("Optional static port. If omitted, a random open port is chosen.");
    let interactive_shell_arg = Arg::with_name("shell")
        .short("s")
        .long("shell")
        .takes_value(false)
        .help("Optionally starts a psql shell.");
    let pgbouncer_arg = Arg::with_name("pgbouncer")
        .long("pgbouncer")
        .value_name("PGBOUNCER")
        .takes_value(false)
        .help("Enable pgbouncer usage for tunnelling. If not provided with kubectl or some other method. Not compatible with --shell");
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
        .global(true)
        .short("c")
        .long("config")
        .value_name("FILE")
        .takes_value(true)
        .default_value("default")
        .help("Config name to read toml configuration from.");
    let force_arg = Arg::with_name("force")
        .required(false)
        .long("force")
        .short("f")
        .takes_value(false)
        .help("Force the action without prompting for confirmation");

    let app = App::new(format!("{} - Figure development cli tools", env!("CARGO_PKG_NAME")))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .arg(config_arg)
        .subcommand(SubCommand::with_name(DOCTOR)
            .about("Checks if all required dependencies are installed")
        )
        .subcommand(SubCommand::with_name(CONFIG)
            .about("Configuration related operations (list environments, etc.)")
            .subcommand(SubCommand::with_name(CHECK)
                .about("Checks if a configuration file exists for the current directory")
            )
            .subcommand(SubCommand::with_name(EDIT)
                .about("Opens the configuration file for the current directory using the system editor")
            )
            .subcommand(SubCommand::with_name(INIT)
                .arg(&force_arg)
                .arg(&Arg::with_name("from")
                    .required(false)
                    .long("from")
                    .value_name("FILE")
                    .takes_value(true)
                    .help("Copy an existing configuration file")
                )
                .about("Installs a stub configuration file with examples to help with setup")
            )
            .subcommand(SubCommand::with_name(PATH)
                .about("Prints the location of the configuration file that will be used")
            )
            .subcommand(SubCommand::with_name(SHOW)
                .about("Prints the contents of the configuration file that will be used")
            )
            .subcommand(SubCommand::with_name(LIST)
                .arg(&Arg::with_name("all")
                    .required(false)
                    .long("all")
                    .short("A")
                    .takes_value(false)
                    .help("List all configuration files regardless of the current directory")
                )
                .about("List configurations available for the current directory")
            )
        )
        .subcommand(SubCommand::with_name(PORT_FORWARD)
            .arg(&Arg::with_name("forward")
                 .value_name("SPECIFIER")
                 .required(true)
                 .takes_value(true)
                 .help("Forwarding specifier string, like the ssh -L option")
                 .long_help("The forwarding string is meant to function like ssh's -L option:\n\n\
                              - <remote-host>:<remote-port>\n- <local-port>:<remote-host>:<remote-port>\n\n\
                              If <local-port> is omitted, a random port will be chosen\n")
            )
            .arg(&Arg::with_name("context")
                 .required(false)
                 .value_name("NAME")
                 .long("context")
                 .takes_value(true)
                 .help("The Kubernetes context to use. Overrides the one provided in config")
            )
            .arg(&Arg::with_name("namespace")
                 .required(false)
                 .value_name("NAME")
                 .long("namespace")
                 .short("n")
                 .takes_value(true)
                 .help("The kubernetes namespace to use. Overrides the one provided in config")
            )
            .about("Perform port forwarding within a Kubernetes cluster")
        )
        .subcommand(SubCommand::with_name(POSTGRES_CLI)
            .arg(&env_arg)
            .arg(&static_port_arg)
            .arg(&interactive_shell_arg)
            .arg(&pgbouncer_arg)
            .about("Proxies a remote postgres connection")
        )
        .subcommand(SubCommand::with_name(KONG_API_KEY)
            .arg(&Arg::with_name("name")
                 .required(true)
                 .value_name("NAME")
                 .long("name")
                 .short("p")
                 .takes_value(true)
                 .help("The string used for naming resources.")
                 .long_help(r#"The string used for naming resources. This naming convention is to
include both the caller and what system they are calling, i.e.
"third-party-to-service-identity"."#)
            )
            .arg(&Arg::with_name("namespace")
                 .required(true)
                 .value_name("NAMESPACE")
                 .long("namespace")
                 .short("n")
                 .takes_value(true)
                 .help("The kubernetes namespace to use.")
            )
            .arg(&Arg::with_name("acl-group")
                 .required(true)
                 .value_name("ACL")
                 .long("acl-group")
                 .short("a")
                 .takes_value(true)
                 .help("The acl group name to use.")
            )
            .arg(&Arg::with_name("uuid")
                 .required(false)
                 .value_name("UUID")
                 .long("uuid")
                 .short("u")
                 .takes_value(true)
                 .default_value(&rand_uuid)
                 .help("Uuid to use during header exchange. Defaults to a random UUID")
            )
            .about("Creates a new kong api key")
        );

    let mut app_help = app.clone();
    let args = app.get_matches();

    let mut config_path = default_config_path.clone();
    config_path.push(args.value_of("config").unwrap());
    config_path.set_extension("toml");

    match args.subcommand() {
        (DOCTOR, _) => {
            let commands = vec![
                doctor_cmd("kubectl", vec![""]),
                doctor_cmd("psql", vec!["--version"]),
                doctor_cmd("gcloud", vec!["version"]),
                doctor_cmd("pgbouncer", vec!["--version"]),
            ];

            if commands.iter().any(|res| res.is_err()) {
                return Err(FigError::DoctorError(
                    "Please make sure all of the above checks are successful!".to_owned(),
                ));
            }
        }
        // <<<<<<< HEAD
        (CONFIG, Some(values)) => match values.subcommand() {
            (CHECK, _) => config_show_path(config_path, true)?,
            (EDIT, _) => config_edit_path(config_path)?,
            (INIT, Some(init)) => config_init_cmd(
                config_path,
                init.is_present("force"),
                init.value_of("from")
                    .map(|p| (PathBuf::from(p), base_config_path)),
            )?,
            (LIST, Some(list)) => config_list_files(if list.is_present("all") {
                &base_config_path
            } else {
                &default_config_path
            })?,
            (PATH, _) => config_show_path(config_path, false)?,
            (SHOW, _) => config_show_contents(config_path)?,
            _ => {
                app_help.print_help().unwrap();
                // =======
                //         (CONFIG, Some(values)) => {
                //             let (mut config_path, base_config_path) = get_config_paths()?;
                //             let default_config_path = config_path.clone();
                //             config_path.push(args.value_of("config").unwrap());
                //             config_path.set_extension("toml");

                //             match values.subcommand() {
                //                 (CHECK, _) => config_show_path(config_path, true)?,
                //                 (EDIT, _) => config_edit_path(config_path)?,
                //                 (INIT, Some(init)) => config_init_cmd(
                //                     config_path,
                //                     init.is_present("force"),
                //                     init.value_of("from")
                //                         .map(|p| (PathBuf::from(p), base_config_path)),
                //                 )?,
                //                 (LIST, Some(list)) => config_list_files(if list.is_present("all") {
                //                     &base_config_path
                //                 } else {
                //                     &default_config_path
                //                 })?,
                //                 (PATH, _) => config_show_path(config_path, false)?,
                //                 (SHOW, _) => config_show_contents(config_path)?,
                //                 _ => {
                //                     app_help.print_help().unwrap();
                //                 }
                // >>>>>>> 2b2875f413b01c8adf7a50e530b96a194509a47d
            }
        },
        (PORT_FORWARD, Some(values)) => {
            let (mut config_path, _) = get_config_paths()?;
            config_path.push(args.value_of("config").unwrap());
            config_path.set_extension("toml");

            let port_forward_config = get_config(config_path).ok().and_then(|c| c.port_forward);
            let forward_value = values
                .value_of("forward")
                .ok_or_else(|| FigError::ParseError("Could not parse remote string".to_owned()))?;
            let forwarding = util::parse_forwarding_string(forward_value)?;
            let context = values.value_of("context");
            let namespace = values.value_of("namespace");

            k8s_port_forward(
                port_forward_config.as_ref(),
                &forwarding,
                context,
                namespace,
            )?
        }
        (POSTGRES_CLI, Some(values)) => {
            let (mut config_path, _) = get_config_paths()?;
            config_path.push(args.value_of("config").unwrap());
            config_path.set_extension("toml");

            // TODO on this error make sure printed messages shows you how to create a config file
            let config = get_config(config_path)?;
            let port = match value_t!(values.value_of("port"), u16) {
                Ok(port) => Ok(Some(port)),
                Err(e) => match e.kind {
                    clap::ErrorKind::ArgumentNotFound => Ok(None),
                    clap::ErrorKind::InvalidValue => Err(FigError::ParseError(
                        "Could not parse port to u16.".to_owned(),
                    )),
                    // TDOO figure out which error conditions we need to add
                    _ => unreachable!(),
                },
            }?;
            let interactive_shell = values.is_present("shell");
            let use_pgbouncer = values.is_present("pgbouncer");

            postgres_cli_cmd(
                &config,
                values.value_of("environment"),
                port,
                interactive_shell,
                use_pgbouncer,
            )?
        }
        (KONG_API_KEY, Some(values)) => {
            let uuid = Uuid::try_parse(values.value_of("uuid").unwrap())?;
            let name = values.value_of("name").unwrap();
            let namespace = values.value_of("namespace").unwrap();
            let acl_group = values.value_of("acl-group").unwrap();

            generate_kong_api_keys(namespace, name, acl_group, &uuid)?;
        }
        _ => {
            // print help by default:
            app_help.print_help().unwrap();
        }
    }

    Ok(())
}

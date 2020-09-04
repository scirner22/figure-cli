use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug)]
pub struct Config {
    k8s_test: Option<K8sConfig>,
    k8s_prod: Option<K8sConfig>,

    log_test: Option<LogConfig>,
    log_prod: Option<LogConfig>,

    exec_test: Option<ExecConfig>,
    exec_prod: Option<ExecConfig>,

    postgres_local: Option<PostgresConfig>,
    postgres_test: Option<PostgresConfig>,
    postgres_prod: Option<PostgresConfig>,
}

#[derive(Deserialize, Debug)]
pub struct PostgresConfig {
    cluster: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    user: String,
    password: String,
    database: String,
    schema: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct K8sConfig {
    deployment: String,
}

#[derive(Deserialize, Debug)]
pub struct ExecConfig {
    cmd: String,
}

#[derive(Deserialize, Debug)]
pub struct LogConfig {
    follow: bool,
}

#[derive(Deserialize, Eq, Hash, PartialEq)]
pub enum EnvironmentType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "prod")]
    Production,
}

pub fn environment_type(env: Option<&str>) -> EnvironmentType {
    match env {
        Some(crate::LOCAL) => EnvironmentType::Local,
        Some(crate::TEST) => EnvironmentType::Test,
        Some(crate::PRODUCTION) => EnvironmentType::Production,
        _ => EnvironmentType::Local,
    }
}

pub fn get_config<P: AsRef<Path>>(path: P) -> crate::Result<Config> {
    let toml_string = fs::read_to_string(path)?;

    toml::from_str(&toml_string)
        .map_err(From::from)
}

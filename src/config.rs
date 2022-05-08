use serde::Deserialize;
use std::fs;
use std::path::Path;
use crate::FigError::EnvError;

#[derive(Deserialize, Debug)]
pub struct Config {
    // k8s_test: Option<K8sConfig>,
    // k8s_prod: Option<K8sConfig>,

    // log_test: Option<LogConfig>,
    // log_prod: Option<LogConfig>,

    // exec_test: Option<ExecConfig>,
    // exec_prod: Option<ExecConfig>,

    pub port_forward: Option<PortForwardConfig>,

    pub postgres_local: Option<PostgresConfig>,
    pub postgres_test: Option<PostgresConfig>,
    pub postgres_prod: Option<PostgresConfig>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PostgresConfigType {
    Kubernetes { context: String, namespace: String, deployment: String },
    GCloudProxy { instance: String },
    Direct,
}

#[derive(Deserialize, Debug)]
pub struct PostgresConfig {
    #[serde(rename = "type")]
    pub _type: PostgresConfigType,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: String,
    pub password: String,
    pub database: String,
    pub schema: Option<String>,
}

impl PostgresConfig {
    pub fn host(&self) -> String {
        self.host.clone().unwrap_or_else(|| "localhost".to_owned())
    }

    pub fn port(&self) -> u16 {
        self.port.unwrap_or(5432)
    }

    pub fn schema(&self) -> String {
        self.schema.clone().unwrap_or_else(|| "public".to_owned())
    }
}

#[derive(Deserialize, Debug)]
pub struct PortForwardConfig {
    pub context: String,
    pub namespace: Option<String>
}

// #[derive(Deserialize, Debug)]
// pub struct K8sConfig {
//     deployment: String,
// }

// #[derive(Deserialize, Debug)]
// pub struct ExecConfig {
//     cmd: String,
// }

// #[derive(Deserialize, Debug)]
// pub struct LogConfig {
//     follow: bool,
// }

#[derive(Deserialize, Eq, Hash, PartialEq)]
pub enum EnvironmentType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "prod")]
    Production,
}

pub fn environment_type(env: Option<&str>) -> crate::Result<EnvironmentType> {
    match env {
        Some(crate::LOCAL) => Ok(EnvironmentType::Local),
        Some(crate::TEST) => Ok(EnvironmentType::Test),
        Some(crate::PRODUCTION) => Ok(EnvironmentType::Production),
        _ => Err(EnvError("environment not set".to_owned()))
    }
}

pub fn get_config<P: AsRef<Path>>(path: P) -> crate::Result<Config> {
    let toml_string = fs::read_to_string(path)?;

    toml::from_str(&toml_string)
        .map_err(From::from)
}

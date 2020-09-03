use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug)]
pub struct Config {
    postgres_local: Option<PostgresConfig>,
    postgres_test: Option<PostgresConfig>,
    postgres_prod: Option<PostgresConfig>,
}

#[derive(Deserialize, Debug)]
pub struct PostgresConfig {
    host: Option<String>,
    port: Option<u16>,
    user: String,
    password: String,
    database: String,
    schema: Option<String>,
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

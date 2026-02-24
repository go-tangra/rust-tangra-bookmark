use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSection,
}

#[derive(Debug, Deserialize)]
pub struct ServerSection {
    pub grpc: GrpcConfig,
    #[serde(default)]
    pub http: Option<HttpConfig>,
}

#[derive(Debug, Deserialize)]
pub struct HttpConfig {
    pub addr: String,
}

#[derive(Debug, Deserialize)]
pub struct GrpcConfig {
    pub addr: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_timeout() -> String {
    "30s".to_string()
}

#[derive(Debug, Deserialize)]
pub struct DataConfig {
    pub data: DataSection,
}

#[derive(Debug, Deserialize)]
pub struct DataSection {
    pub database: DatabaseConfig,
    #[serde(default)]
    pub redis: Option<RedisConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_driver")]
    pub driver: String,
    pub source: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

fn default_driver() -> String {
    "postgresql".to_string()
}

fn default_max_connections() -> u32 {
    20
}

#[derive(Debug, Deserialize)]
pub struct RedisConfig {
    pub addr: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub db: u8,
}

#[derive(Debug, Deserialize)]
pub struct LoggerConfig {
    pub logger: LoggerSection,
}

#[derive(Debug, Deserialize)]
pub struct LoggerSection {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_output")]
    pub output: String,
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_level() -> String {
    "info".to_string()
}

fn default_output() -> String {
    "stdout".to_string()
}

fn default_format() -> String {
    "json".to_string()
}

pub fn load_config<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let content = std::fs::read_to_string(path)?;
    let config: T = serde_yaml::from_str(&content)?;
    Ok(config)
}

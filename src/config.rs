use kroeg_server::config::ServerConfig;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct KroegConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "backend")]
pub enum DatabaseConfig {
    #[serde(rename = "postgresql")]
    PostgreSQL {
        server: String,
        username: String,
        password: String,
        database: String,
    },
}

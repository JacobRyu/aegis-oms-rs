use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct AccountConfig {
    pub initial_balance: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub max_order_quantity: Decimal,
    pub max_open_orders: usize,
    pub max_open_positions: usize,
    pub stop_out_ratio: Decimal,
    pub margin_call_ratio: Decimal,
    pub max_loss: Option<Decimal>,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_order_quantity: Decimal::new(1_000_000, 0),
            max_open_orders: 100,
            max_open_positions: 20,
            stop_out_ratio: Decimal::new(50, 0),
            margin_call_ratio: Decimal::new(100, 0),
            max_loss: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: String,
}

impl SmtpConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            host: std::env::var("SMTP_HOST").ok()?,
            port: std::env::var("SMTP_PORT").ok()?.parse().ok()?,
            username: std::env::var("SMTP_USER").ok()?,
            password: std::env::var("SMTP_PASS").ok()?,
            from: std::env::var("ALERT_FROM").ok()?,
            to: std::env::var("ALERT_TO").ok()?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub account: AccountConfig,
    pub risk: RiskConfig,
    pub smtp: Option<SmtpConfig>,
}

impl AppConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {e}"))
    }

    pub fn default_with_balance(balance: Decimal) -> Self {
        Self {
            account: AccountConfig { initial_balance: balance },
            risk: RiskConfig::default(),
            smtp: SmtpConfig::from_env(),
        }
    }

    pub fn load() -> Self {
        let config_path =
            std::env::var("AEGIS_CONFIG").unwrap_or_else(|_| "config/default.toml".to_string());
        if Path::new(&config_path).exists() {
            Self::from_file(&config_path).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Failed to load config file, using defaults");
                Self::default_with_balance(Decimal::new(100_000, 0))
            })
        } else {
            Self::default_with_balance(Decimal::new(100_000, 0))
        }
    }
}

use std::env;
use std::str::FromStr;
use rust_decimal::Decimal;
use dotenv::dotenv;
use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
    pub private_key: String,
    pub funder_address: String,
    pub http_url: String,
    pub ws_url: String,
    pub max_daily_loss_pct: Decimal,
    pub max_trade_capital_pct: Decimal,
    pub min_edge: Decimal,
    pub poll_interval_ms: u64,
    // Safety & Re-entry
    pub min_liquidity_multiplier: Decimal, // 5.0
    pub normalization_threshold: Decimal, // 0.99
    pub normalization_updates: u32, // 3
    pub trade_cooldown_ms: i64, // 30000
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenv().ok();

        let max_daily_loss_pct = Decimal::from_str(
            &env::var("MAX_DAILY_LOSS_PCT").unwrap_or_else(|_| "0.02".to_string())
        ).context("Invalid MAX_DAILY_LOSS_PCT")?;

        let max_trade_capital_pct = Decimal::from_str(
            &env::var("MAX_TRADE_CAPITAL_PCT").unwrap_or_else(|_| "0.01".to_string())
        ).context("Invalid MAX_TRADE_CAPITAL_PCT")?;

        let min_edge = Decimal::from_str(
            &env::var("MIN_EDGE").unwrap_or_else(|_| "0.05".to_string())
        ).context("Invalid MIN_EDGE")?;

        Ok(Self {
            api_key: env::var("POLY_API_KEY").context("POLY_API_KEY must be set")?,
            api_secret: env::var("POLY_API_SECRET").context("POLY_API_SECRET must be set")?,
            api_passphrase: env::var("POLY_API_PASSPHRASE").context("POLY_API_PASSPHRASE must be set")?,
            private_key: env::var("POLY_PRIVATE_KEY").context("POLY_PRIVATE_KEY must be set")?,
            funder_address: env::var("POLY_FUNDER").context("POLY_FUNDER must be set")?,
            http_url: env::var("POLY_HTTP_URL").unwrap_or_else(|_| "https://clob.polymarket.com".to_string()),
            ws_url: env::var("POLY_WS_URL").unwrap_or_else(|_| "wss://clob.polymarket.com/ws/".to_string()),
            max_daily_loss_pct,
            max_trade_capital_pct,
            min_edge,
            poll_interval_ms: 250,
            min_liquidity_multiplier: Decimal::new(5, 0),
            normalization_threshold: Decimal::new(99, 2), // 0.99
            normalization_updates: 3,
            trade_cooldown_ms: 30000, // 30 seconds
        })
    }
}

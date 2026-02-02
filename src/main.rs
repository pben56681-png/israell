mod config;
mod types;
mod market;
mod execution;
mod risk;
mod strategy;

use config::Config;
use risk::RiskManager;
use market::MarketMonitor;
use execution::ExecutionEngine;
use strategy::StrategyEngine;
use std::sync::Arc;
use tracing::{info, error};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    info!("Starting Polymarket Binary Arbitrage Bot...");

    // 2. Load Config
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {}", e);
            return Ok(());
        }
    };
    info!("Config loaded. Max Daily Loss: {}%", config.max_daily_loss_pct * rust_decimal::Decimal::from(100));

    // 3. Initialize Components
    // Mock initial balance of 1000 USDC
    let initial_balance = rust_decimal::Decimal::from(1000);
    let risk_manager = RiskManager::new(
        initial_balance, 
        config.max_daily_loss_pct, 
        config.max_trade_capital_pct
    );

    let market_monitor = Arc::new(MarketMonitor::new(config.clone()));
    let execution_engine = Arc::new(ExecutionEngine::new(config.clone(), risk_manager.clone()));
    let strategy_engine = StrategyEngine::new(market_monitor.clone(), execution_engine.clone(), config.clone());

    // 4. Start Background Tasks
    
    // Start market discovery (Mocked)
    market_monitor.start_market_discovery().await;
    
    // Start WebSocket Loop
    let monitor_clone = market_monitor.clone();
    tokio::spawn(async move {
        monitor_clone.run_ws_loop().await;
    });

    // 5. Run Strategy Loop (Event Driven)
    strategy_engine.run().await;

    Ok(())
}

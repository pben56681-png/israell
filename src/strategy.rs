use crate::market::MarketMonitor;
use crate::execution::ExecutionEngine;
use crate::config::Config;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn, error};
use tokio::sync::broadcast;
use chrono::Utc;

const TAKER_FEE: Decimal = Decimal::ZERO; 

pub struct StrategyEngine {
    market_monitor: Arc<MarketMonitor>,
    execution_engine: Arc<ExecutionEngine>,
    config: Config,
}

impl StrategyEngine {
    pub fn new(market_monitor: Arc<MarketMonitor>, execution_engine: Arc<ExecutionEngine>, config: Config) -> Self {
        Self {
            market_monitor,
            execution_engine,
            config,
        }
    }

    pub async fn run(&self) {
        info!("Strategy Engine Started. Waiting for WS updates...");
        
        let mut rx = self.market_monitor.update_tx.subscribe();

        loop {
            match rx.recv().await {
                Ok(market_id) => {
                    self.process_market_update(&market_id).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Strategy lagged behind {} updates", n);
                }
                Err(e) => {
                    error!("Strategy loop error: {}", e);
                    break;
                }
            }
        }
    }

    async fn process_market_update(&self, market_id: &str) {
        // 1. Get Tokens
        let (yes_token, no_token) = match self.market_monitor.get_market_tokens(market_id) {
            Some(t) => t,
            None => return,
        };
        
        // 2. Check Liquidity (Fast Fail)
        // Order size currently static 10.0, ideally dynamic.
        let trade_size = Decimal::new(10, 0); 
        let required_liquidity = trade_size * self.config.min_liquidity_multiplier;
        
        if !self.market_monitor.check_liquidity(&yes_token, &no_token, required_liquidity) {
             return; // Skip if not enough depth
        }
        
        // 3. Check Re-Entry Safety (Normalization & Cooldown)
        if let Some(state) = self.market_monitor.get_market_state_clone(market_id) {
            // Check Normalized Flag
            if !state.is_normalized {
                return; // Market hasn't normalized since last trade/startup
            }
            
            // Check Cooldown
            if let Some(last_trade) = state.last_trade_time {
                let now = Utc::now();
                let elapsed = now.signed_duration_since(last_trade).num_milliseconds();
                if elapsed < self.config.trade_cooldown_ms {
                     return; // Still cooling down
                }
            }
        }

        // 4. Check Edge (First Pass)
        if let Some((price_yes, price_no)) = self.market_monitor.get_best_asks(&yes_token, &no_token) {
            if self.check_opportunity(price_yes, price_no) {
                // 5. Opportunity Detected. Prepare to Execute.
                
                // Pre-flight Edge Confirmation
                if let Some((final_yes, final_no)) = self.market_monitor.get_best_asks(&yes_token, &no_token) {
                     if self.check_opportunity(final_yes, final_no) {
                        
                        // Mark trade as executing to prevent double-fire
                        // Ideally we lock, but here we just reset the flag after successful execution call
                        // Note: execution_engine.execute_arb calls record_pnl etc.
                        // We need to update market state (reset normalized flag)
                        
                        info!("EXECUTING TRADE on {}: YES @ {}, NO @ {}", market_id, final_yes, final_no);
                        
                        let status = self.execution_engine.execute_arb(
                            market_id,
                            &yes_token,
                            &no_token,
                            final_yes,
                            final_no,
                            trade_size
                        ).await;
                        
                        match status {
                            crate::types::TradeStatus::Filled => {
                                self.market_monitor.mark_trade_executed(market_id);
                                info!("Trade Filled. Cooldown started for {}", market_id);
                            },
                            _ => {
                                warn!("Trade failed or partial fill. Market state preserved (or handled by Risk).");
                            }
                        }

                     } else {
                         warn!("Pre-flight check failed for market {}", market_id);
                     }
                }
            }
        }
    }

    fn check_opportunity(&self, price_yes: Decimal, price_no: Decimal) -> bool {
        let fee_multiplier = Decimal::ONE + TAKER_FEE;
        let cost_yes = price_yes * fee_multiplier;
        let cost_no = price_no * fee_multiplier;
        
        let total_cost = cost_yes + cost_no;
        let edge = Decimal::ONE - total_cost;

        if edge >= self.config.min_edge {
            // Don't log every micro-opportunity, only significant ones or rate limited
            // info!("Arb Opportunity: YES {} + NO {} = {} | Edge: {}", price_yes, price_no, total_cost, edge);
            return true;
        }
        
        false
    }
}

use rust_decimal::Decimal;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct RiskManager {
    state: Arc<Mutex<RiskState>>,
    max_daily_loss_pct: Decimal,
    max_trade_capital_pct: Decimal,
}

#[derive(Debug)]
struct RiskState {
    initial_balance: Decimal,
    current_balance: Decimal,
    daily_pnl: Decimal,
    safe_mode: bool,
}

impl RiskManager {
    pub fn new(initial_balance: Decimal, max_daily_loss_pct: Decimal, max_trade_capital_pct: Decimal) -> Self {
        Self {
            state: Arc::new(Mutex::new(RiskState {
                initial_balance,
                current_balance: initial_balance,
                daily_pnl: Decimal::ZERO,
                safe_mode: false,
            })),
            max_daily_loss_pct,
            max_trade_capital_pct,
        }
    }

    pub fn check_trade_size(&self, required_amount: Decimal) -> bool {
        let state = self.state.lock().unwrap();
        
        if state.safe_mode {
            warn!("Risk Check Failed: SAFE MODE is active.");
            return false;
        }

        let max_trade_size = state.current_balance * self.max_trade_capital_pct;
        if required_amount > max_trade_size {
            warn!("Risk Check Failed: Trade size {} exceeds limit {}", required_amount, max_trade_size);
            return false;
        }

        // Check daily loss limit
        let loss_limit = state.initial_balance * self.max_daily_loss_pct;
        if state.daily_pnl < -loss_limit {
             warn!("Risk Check Failed: Daily loss limit reached.");
             return false;
        }

        true
    }

    pub fn record_pnl(&self, pnl: Decimal) {
        let mut state = self.state.lock().unwrap();
        state.daily_pnl += pnl;
        state.current_balance += pnl;
        
        info!("PnL Updated: Daily PnL: {}, Balance: {}", state.daily_pnl, state.current_balance);

        let loss_limit = state.initial_balance * self.max_daily_loss_pct;
        if state.daily_pnl < -loss_limit {
            error!("CRITICAL: Daily loss limit hit! Entering SAFE MODE.");
            state.safe_mode = true;
        }
    }

    pub fn enter_safe_mode(&self) {
        let mut state = self.state.lock().unwrap();
        state.safe_mode = true;
        error!("Manual trigger: Entering SAFE MODE.");
    }
    
    pub fn is_safe_mode(&self) -> bool {
        self.state.lock().unwrap().safe_mode
    }
}

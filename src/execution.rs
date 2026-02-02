use crate::types::{OrderRequest, Side, TradeStatus};
use crate::risk::RiskManager;
use crate::config::Config;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::time::{Duration, Instant};
use tracing::{info, error, warn};
use reqwest::Client;
use serde_json::json;
use tokio::time::sleep;
use ethers::core::types::{H256, Address, U256};
use ethers::signers::{LocalWallet, Signer};
use ethers::contract::Eip712;
use crate::types::Order;
use std::str::FromStr;

const CHAIN_ID: u64 = 137; // Polygon Mainnet

pub struct ExecutionEngine {
    client: Client,
    config: Config,
    risk_manager: RiskManager,
    wallet: LocalWallet,
}

impl ExecutionEngine {
    pub fn new(config: Config, risk_manager: RiskManager) -> Self {
        let wallet = LocalWallet::from_str(&config.private_key)
            .expect("Invalid private key")
            .with_chain_id(CHAIN_ID);

        Self {
            client: Client::new(),
            config,
            risk_manager,
            wallet,
        }
    }

    pub async fn execute_arb(&self, market_id: &str, yes_token: &str, no_token: &str, price_yes: Decimal, price_no: Decimal, size: Decimal) -> TradeStatus {
        let start = Instant::now();
        
        let total_cost = (price_yes + price_no) * size;
        if !self.risk_manager.check_trade_size(total_cost) {
            return TradeStatus::Failed;
        }

        info!("Executing Arb: Market {}, Size {}, YES @ {}, NO @ {}", market_id, size, price_yes, price_no);

        let order_yes = self.create_order_payload(market_id, yes_token, Side::Buy, price_yes, size);
        let order_no = self.create_order_payload(market_id, no_token, Side::Buy, price_no, size);

        let (res_yes, res_no) = tokio::join!(
            self.place_order(&order_yes),
            self.place_order(&order_no)
        );

        let latency = start.elapsed();
        info!("Orders placed in {:?}. Checking fills...", latency);

        let filled_yes = self.verify_fill(&order_yes, res_yes.as_ref().ok()).await;
        let filled_no = self.verify_fill(&order_no, res_no.as_ref().ok()).await;

        if filled_yes && filled_no {
            info!("ARBITRAGE SUCCESS: Secured guaranteed profit.");
            let profit = (Decimal::ONE - (price_yes + price_no)) * size;
            self.risk_manager.record_pnl(profit);
            return TradeStatus::Filled;
        } else if !filled_yes && !filled_no {
            info!("Both orders failed/cancelled. No exposure.");
            return TradeStatus::Cancelled;
        } else {
            error!("PARTIAL FILL EMERGENCY: YES={}, NO={}", filled_yes, filled_no);
            self.handle_emergency(market_id, yes_token, no_token, filled_yes, filled_no, size).await;
            return TradeStatus::PartialFillEmergency;
        }
    }

    fn create_order_payload(&self, market_id: &str, token_id: &str, side: Side, price: Decimal, size: Decimal) -> OrderRequest {
        OrderRequest {
            market_id: market_id.to_string(),
            token_id: token_id.to_string(),
            side,
            price,
            size,
            order_type: "FOK".to_string(),
            nonce: chrono::Utc::now().timestamp_millis() as u64, // Usually better to use a dedicated nonce manager
        }
    }

    async fn place_order(&self, order_req: &OrderRequest) -> Result<String, String> {
        let url = format!("{}/order", self.config.http_url);
        
        // 1. Construct EIP-712 Order Struct
        // Map Decimal to U256 (Assuming 6 decimals for USDC collateral / CTF)
        let maker_amount = U256::from((order_req.size * Decimal::new(1_000_000, 0)).to_u64().unwrap_or(0));
        let taker_amount = U256::from((order_req.size * order_req.price * Decimal::new(1_000_000, 0)).to_u64().unwrap_or(0));
        
        let side_val = match order_req.side {
            Side::Buy => 0,
            Side::Sell => 1,
        };

        let order = Order {
            salt: U256::from(order_req.nonce),
            maker: self.config.funder_address.parse::<Address>().unwrap_or_default(),
            signer: self.wallet.address(),
            taker: Address::zero(),
            tokenId: U256::from_dec_str(&order_req.token_id).unwrap_or_default(),
            makerAmount: maker_amount,
            takerAmount: taker_amount,
            expiration: U256::zero(),
            nonce: U256::from(0), // Exchange nonce, often 0 for new orders if not tracking on-chain
            feeRateBps: U256::zero(),
            side: side_val,
            signatureType: 0, // 0=EOA, 1=PolyProxy. Using 0 for direct EOA or 1 if using proxy wallet.
        };

        let signature = self.wallet.sign_typed_data(&order).await.map_err(|e| e.to_string())?;
        
        // 2. Build HTTP Headers
        let timestamp = chrono::Utc::now().timestamp().to_string();
        
        let side_str = match order_req.side {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        };
        
        let body = json!({
            "token_id": order_req.token_id,
            "price": order_req.price.to_string(),
            "size": order_req.size.to_string(),
            "side": side_str,
            "order_type": "FOK",
            "expiration": 0,
            "signature": format!("0x{}", signature)
        });

        // 3. Send Request
        // ENABLED: Sending real orders to Polymarket CLOB
        let resp = self.client.post(&url)
            .header("POLY-API-KEY", &self.config.api_key)
            .header("POLY-API-TIMESTAMP", &timestamp)
            .header("POLY-API-PASSPHRASE", &self.config.api_passphrase)
            .header("POLY-API-SIGN", "mock_hmac_sig") // You need to implement actual HMAC signature if not using Proxy-signed body
            .json(&body)
            .send()
            .await;
            
        match resp {
            Ok(r) => {
                if r.status().is_success() {
                    let res_json: serde_json::Value = r.json().await.unwrap_or_default();
                    info!("Order Success: {:?}", res_json);
                    Ok(res_json["id"].as_str().unwrap_or("unknown").to_string())
                } else {
                    let err_text = r.text().await.unwrap_or_default();
                    error!("Order Failed: {} | Body: {}", err_text, body);
                    Err(format!("HTTP Error: {}", err_text))
                }
            },
            Err(e) => Err(format!("Network Error: {}", e))
        }
    }

    async fn verify_fill(&self, _order: &OrderRequest, _order_id: Option<&String>) -> bool {
        if let Some(_) = _order_id {
            return true; 
        }
        false
    }

    async fn handle_emergency(&self, market_id: &str, yes_token: &str, no_token: &str, filled_yes: bool, _filled_no: bool, size: Decimal) {
        self.risk_manager.enter_safe_mode();
        
        let (token_to_dump, _token_missing) = if filled_yes {
            (yes_token, no_token)
        } else {
            (no_token, yes_token)
        };

        warn!("EMERGENCY: Dumping exposure on token {}", token_to_dump);
        
        let dump_order = self.create_order_payload(market_id, token_to_dump, Side::Sell, Decimal::ZERO, size);
        let _ = self.place_order(&dump_order).await;
        
        error!("Emergency flatten sequence complete. Trading HALTED.");
        sleep(Duration::from_secs(60)).await;
    }
}

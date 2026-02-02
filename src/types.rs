use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use ethers::types::{Address, U256};
use ethers::contract::{Eip712, EthAbiType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketResponse {
    pub data: Vec<Market>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub condition_id: String,
    pub question: String,
    pub tokens: Vec<Token>,
    pub active: bool,
    pub closed: bool,
    pub accepting_orders: bool,
    pub end_date_iso: Option<String>,
    pub tags: Option<Vec<String>>,
    // Local state fields (not from API)
    #[serde(skip, default)]
    pub state: MarketState,
}

#[derive(Debug, Clone, Default)]
pub struct MarketState {
    pub last_trade_time: Option<DateTime<Utc>>,
    pub is_normalized: bool,
    pub consecutive_normalized_updates: u32,
    pub last_edge: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub token_id: String,
    pub outcome: String, // "Yes" or "No"
    pub price: Decimal,
    pub winner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub market_id: String, 
    pub asset_id: String,
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Level {
    pub price: Decimal,
    pub size: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub market_id: String,
    pub token_id: String,
    pub side: Side,
    pub price: Decimal,
    pub size: Decimal,
    pub order_type: String, // "FOK"
    pub nonce: u64,
}

// EIP-712 Structs
#[derive(Debug, Clone, Eip712, EthAbiType)]
#[eip712(
    name = "Polymarket CTF Exchange",
    version = "1",
    chain_id = 137,
    verifying_contract = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E"
)]
pub struct Order {
    pub salt: U256,
    pub maker: Address,
    pub signer: Address,
    pub taker: Address,
    pub tokenId: U256,
    pub makerAmount: U256,
    pub takerAmount: U256,
    pub expiration: U256,
    pub nonce: U256,
    pub feeRateBps: U256,
    pub side: u8, // 0 for Buy, 1 for Sell
    pub signatureType: u8, // 0 for EOA, 1 for Poly Proxy, 2 for Kernel
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeEvent {
    pub id: Uuid,
    pub market_id: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub edge: Decimal,
    pub timestamp: DateTime<Utc>,
    pub status: TradeStatus,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TradeStatus {
    Pending,
    Filled,
    PartialFillEmergency,
    Failed,
    Cancelled,
}

// WebSocket Specific Types

#[derive(Debug, Serialize)]
pub struct WsSubscribeMsg {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub asset_ids: Vec<String>,
    pub channels: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event_type")]
pub enum WsMessage {
    #[serde(rename = "book")]
    Book(WsBookUpdate),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct WsBookUpdate {
    pub asset_id: String,
    pub bids: Vec<WsLevel>,
    pub asks: Vec<WsLevel>,
    pub hash: String,
    pub timestamp: String, 
}

#[derive(Debug, Deserialize)]
pub struct WsLevel(String, String); 

impl WsLevel {
    pub fn to_level(&self) -> Option<Level> {
        let price = self.0.parse::<Decimal>().ok()?;
        let size = self.1.parse::<Decimal>().ok()?;
        Some(Level { price, size })
    }
}

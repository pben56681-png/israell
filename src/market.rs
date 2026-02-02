use crate::types::{Market, Token, OrderBook, Level, WsMessage, WsSubscribeMsg, WsLevel, MarketResponse};
use crate::config::Config;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use rust_decimal::Decimal;
use chrono::{Utc, TimeZone};
use reqwest::Client;
use tracing::{info, error, warn, debug};
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use futures_util::{StreamExt, SinkExt};
use std::time::Duration;

pub struct MarketMonitor {
    active_markets: Arc<RwLock<HashMap<String, Market>>>,
    token_to_market: Arc<RwLock<HashMap<String, String>>>, // token_id -> market_id
    order_books: Arc<RwLock<HashMap<String, OrderBook>>>, // token_id -> OrderBook
    client: Client,
    config: Config,
    pub update_tx: broadcast::Sender<String>, // Broadcasts market_id on update
}

impl MarketMonitor {
    pub fn new(config: Config) -> Self {
        let (update_tx, _) = broadcast::channel(100);
        
        Self {
            active_markets: Arc::new(RwLock::new(HashMap::new())),
            token_to_market: Arc::new(RwLock::new(HashMap::new())),
            order_books: Arc::new(RwLock::new(HashMap::new())),
            client: Client::new(),
            config,
            update_tx,
        }
    }

    pub async fn start_market_discovery(&self) {
        info!("Starting market discovery via REST API...");
        
        let url = format!("{}/markets?active=true&limit=100", self.config.http_url);
        
        match self.client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(market_response) = resp.json::<MarketResponse>().await {
                    let mut count = 0;
                    for market in market_response.data {
                        if self.is_valid_crypto_market(&market) {
                            self.add_market(market);
                            count += 1;
                        }
                    }
                    info!("Discovered {} valid crypto markets.", count);
                } else {
                    error!("Failed to parse markets response");
                }
            },
            Err(e) => {
                error!("Failed to fetch markets: {}", e);
            }
        }
    }

    fn is_valid_crypto_market(&self, market: &Market) -> bool {
        if !market.active || !market.accepting_orders {
            return false;
        }

        if market.tokens.len() != 2 {
            return false;
        }

        let is_crypto = if let Some(tags) = &market.tags {
            tags.iter().any(|t| t == "Crypto" || t == "Bitcoin" || t == "Ethereum" || t == "Solana")
        } else {
            false
        };

        if !is_crypto {
            return false;
        }
        
        true
    }

    fn add_market(&self, market: Market) {
        let market_id = market.condition_id.clone();
        let tokens = market.tokens.clone();

        {
            let mut markets = self.active_markets.write().unwrap();
            markets.insert(market_id.clone(), market);
        }
        
        {
            let mut map = self.token_to_market.write().unwrap();
            for token in tokens {
                map.insert(token.token_id, market_id.clone());
            }
        }
    }

    pub async fn run_ws_loop(&self) {
        let url_str = &self.config.ws_url;
        let mut backoff = 1;
        
        loop {
            info!("Connecting to WS: {}", url_str);
            
            let mut request = url_str.into_client_request().expect("Failed to build request");
            let headers = request.headers_mut();
            headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36".parse().unwrap());

            match connect_async(request).await {
                Ok((ws_stream, _)) => {
                    info!("WebSocket Connected");
                    backoff = 1; // Reset backoff on success
                    
                    let (mut write, mut read) = ws_stream.split();

                    // 1. Subscribe
                    let tokens: Vec<String> = {
                        let map = self.token_to_market.read().unwrap();
                        map.keys().cloned().collect()
                    };

                    for chunk in tokens.chunks(50) {
                        let sub_msg = WsSubscribeMsg {
                            msg_type: "subscribe".to_string(),
                            asset_ids: chunk.to_vec(),
                            channels: vec!["book".to_string()],
                        };
                        let json = serde_json::to_string(&sub_msg).unwrap();
                        if let Err(e) = write.send(Message::Text(json)).await {
                            error!("Failed to send subscribe: {}", e);
                            continue;
                        }
                    }
                    info!("Subscribed to {} tokens", tokens.len());

                    // 2. Heartbeat & Read Loop
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(20));
                    
                    loop {
                        tokio::select! {
                            _ = ping_interval.tick() => {
                                // Send Ping
                                if let Err(e) = write.send(Message::Ping(vec![])).await {
                                    error!("Failed to send Ping: {}", e);
                                    break;
                                }
                            }
                            msg = read.next() => {
                                match msg {
                                    Some(Ok(message)) => {
                                        match message {
                                            Message::Text(text) => self.handle_message(&text),
                                            Message::Ping(payload) => {
                                                // Respond to server Ping with Pong
                                                if let Err(e) = write.send(Message::Pong(payload)).await {
                                                     error!("Failed to send Pong: {}", e);
                                                     break;
                                                }
                                            },
                                            Message::Pong(_) => {
                                                // Received pong from server (response to our ping)
                                                debug!("Received Pong");
                                            }, 
                                            Message::Close(frame) => {
                                                warn!("WS Closed by server: {:?}", frame);
                                                break;
                                            },
                                            Message::Binary(_) => {},
                                            Message::Frame(_) => {},
                                        }
                                    }
                                    Some(Err(e)) => {
                                        error!("WS Read Error: {}", e);
                                        break;
                                    }
                                    None => {
                                        warn!("WS Stream Ended");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("WS Connection Failed: {}", e);
                }
            }
            
            // Exponential Backoff
            let wait_secs = std::cmp::min(backoff, 60);
            warn!("Reconnecting in {}s...", wait_secs);
            tokio::time::sleep(Duration::from_secs(wait_secs)).await;
            backoff *= 2;
        }
    }

    fn handle_message(&self, text: &str) {
        if text == "[]" { return; } // Ignore heartbeat
        match serde_json::from_str::<WsMessage>(text) {
            Ok(WsMessage::Book(update)) => {
                // Update local book
                let bids: Vec<Level> = update.bids.iter().filter_map(|l| l.to_level()).collect();
                let asks: Vec<Level> = update.asks.iter().filter_map(|l| l.to_level()).collect();
                
                // Parse timestamp
                let ts = update.timestamp.parse::<i64>().unwrap_or(0);
                let dt = if ts > 2000000000 {
                    Utc.timestamp_millis_opt(ts).single()
                } else {
                    Utc.timestamp_opt(ts, 0).single()
                }.unwrap_or(Utc::now());

                let token_id = update.asset_id.clone();
                
                // Find market_id
                let market_id = {
                    let map = self.token_to_market.read().unwrap();
                    map.get(&token_id).cloned()
                };

                if let Some(mid) = market_id {
                    let book = OrderBook {
                        market_id: mid.clone(),
                        asset_id: token_id.clone(),
                        bids,
                        asks,
                        timestamp: dt,
                    };

                    {
                        let mut books = self.order_books.write().unwrap();
                        books.insert(token_id.clone(), book);
                    }
                    
                    self.update_normalization_state(&mid);
                    let _ = self.update_tx.send(mid);
                }
            }
            Ok(WsMessage::Unknown) => {
                // debug!("Unknown message: {}", text);
            }
            Err(e) => {
                error!("Failed to parse WS message: {} | Text: {}", e, text);
            }
        }
    }
    
    fn update_normalization_state(&self, market_id: &str) {
        let (yes_token, no_token) = match self.get_market_tokens(market_id) {
            Some(t) => t,
            None => return,
        };
        
        if let Some((ask_yes, ask_no)) = self.get_best_asks(&yes_token, &no_token) {
            let sum = ask_yes + ask_no;
            let is_normalized = sum >= self.config.normalization_threshold;
            
            let mut markets = self.active_markets.write().unwrap();
            if let Some(market) = markets.get_mut(market_id) {
                if is_normalized {
                    market.state.consecutive_normalized_updates += 1;
                    if market.state.consecutive_normalized_updates >= self.config.normalization_updates {
                        market.state.is_normalized = true;
                    }
                } else {
                    market.state.consecutive_normalized_updates = 0;
                }
                market.state.last_edge = Decimal::ONE - sum;
            }
        }
    }

    pub fn get_market_tokens(&self, market_id: &str) -> Option<(String, String)> {
        let markets = self.active_markets.read().unwrap();
        if let Some(market) = markets.get(market_id) {
            if market.tokens.len() >= 2 {
                return Some((market.tokens[0].token_id.clone(), market.tokens[1].token_id.clone()));
            }
        }
        None
    }
    
    pub fn get_market_state_clone(&self, market_id: &str) -> Option<crate::types::MarketState> {
         let markets = self.active_markets.read().unwrap();
         markets.get(market_id).map(|m| m.state.clone())
    }
    
    pub fn mark_trade_executed(&self, market_id: &str) {
        let mut markets = self.active_markets.write().unwrap();
        if let Some(market) = markets.get_mut(market_id) {
            market.state.is_normalized = false;
            market.state.consecutive_normalized_updates = 0;
            market.state.last_trade_time = Some(Utc::now());
        }
    }

    pub fn get_best_asks(&self, token_yes: &str, token_no: &str) -> Option<(Decimal, Decimal)> {
        let books = self.order_books.read().unwrap();
        
        let book_yes = books.get(token_yes)?;
        let book_no = books.get(token_no)?;

        let best_ask_yes = book_yes.asks.iter()
            .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal))?;
            
        let best_ask_no = book_no.asks.iter()
            .min_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal))?;

        Some((best_ask_yes.price, best_ask_no.price))
    }
    
    pub fn check_liquidity(&self, token_yes: &str, token_no: &str, required_size: Decimal) -> bool {
        let books = self.order_books.read().unwrap();
        
        let check_token = |token_id: &str| -> bool {
            if let Some(book) = books.get(token_id) {
                 if let Some(best_ask) = book.asks.iter().min_by(|a, b| a.price.partial_cmp(&b.price).unwrap()) {
                     return best_ask.size >= required_size;
                 }
            }
            false
        };
        
        check_token(token_yes) && check_token(token_no)
    }
}

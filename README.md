# Polymarket Binary Arbitrage Bot

A high-performance, async Rust trading bot designed to detect and execute arbitrage opportunities on the Polymarket Central Limit Order Book (CLOB).

## 🚀 Features

*   **Real-Time Data**: Subscribes to Polymarket's WebSocket `book` channel for sub-millisecond updates.
*   **Atomic Execution**: Uses concurrent Fill-Or-Kill (FOK) orders to buy both "Yes" and "No" sides simultaneously when `Price(Yes) + Price(No) < 1.00`.
*   **EIP-712 Authentication**: Signs orders directly with your Ethereum Private Key or L2 Proxy Key (no API keys required).
*   **Safety First**:
    *   **Pre-Flight Checks**: Re-verifies order book state microseconds before execution.
    *   **Normalization Logic**: Only enters markets that have been stable/efficient for a set duration.
    *   **Emergency Flattening**: Automatically dumps exposure if only one leg of the arbitrage fills.
    *   **Circuit Breaker**: Stops trading if daily loss exceeds a configurable threshold (default 2%).

## 🛠️ Prerequisites

*   **Rust**: Stable toolchain installed via [rustup](https://rustup.rs/).
*   **Polymarket Account**:
    *   Must have a Proxy (L2) wallet created.
    *   Must have USDC deposited on Polygon (L2).
    *   Must have approved the Exchange Contract to spend your USDC.

## ⚙️ Configuration

1.  **Clone the repository**:
    ```bash
    git clone https://github.com/yourusername/polymarket-arb-bot.git
    cd polymarket-arb-bot
    ```

2.  **Create a `.env` file**:
    Copy the example or create a new file named `.env` in the root directory. **DO NOT commit this file.**

    ```env
    # Authentication
    POLY_PRIVATE_KEY=your_polygon_private_key
    POLY_FUNDER=your_wallet_address

    # API Endpoints (Production)
    POLY_HTTP_URL=https://clob.polymarket.com
    POLY_WS_URL=wss://ws-subscriptions-clob.polymarket.com/ws/market

    # Legacy API Keys (Optional/If needed for other endpoints)
    POLY_API_KEY=test_key
    POLY_API_SECRET=test_secret
    POLY_API_PASSPHRASE=test_pass

    # Risk Management
    MAX_DAILY_LOSS_PCT=0.02      # Stop if loss > 2%
    MAX_TRADE_CAPITAL_PCT=0.01   # Max 1% of portfolio per trade
    MIN_EDGE=0.05                # Min 5 cents profit per share
    ```

## 🏃 Usage

### Build
```bash
cargo build --release
```

### Run
```bash
cargo run --release
```

The bot will:
1.  Connect to the Polymarket WebSocket.
2.  Discover active Crypto markets (BTC, ETH, SOL).
3.  Listen for order book updates.
4.  Execute arbitrage trades automatically when conditions are met.

## ⚠️ Disclaimer

This software is for educational purposes only. Use it at your own risk. The authors are not responsible for any financial losses incurred while using this bot.

## Polygon arbitrage bot

#### A Rust bot that detects arbitrage opportunities across DEXes like Uniswap and QuickSwap on the Polygon network

### Features
###### 1. Polls token prices of WETH/USDC from quotes.
###### 2. Detects arbitrage opportunities when price differences exceed a threshold.
###### 3. Simulates arbitrage profit including estimated gas cost which is 0.01$ per transaction.
###### 4. Configurable RPC URL, token pairs, and thresholds.
###### 5. Logs simulated opportunities to profit.txt file.

# Polygon Arbitrage Bot

## 4. Installation & Setup

### Prerequisites
- Rust (latest stable version)
- Polygon RPC endpoint (Alchemy, Infura, or public node)
- Cargo package manager

### Clone the Repository
```bash
git clone https://github.com/<your-username>/polygon-arb-bot.git
cd polygon-arb-bot
```

### Install Dependencies
```bash
cargo build
```

### Setup Environment Variables
Create a `.env` file in the project root:

```env
RPC_URL=https://polygon-rpc.com
TOKEN_PAIR=WETH/USDC
DEX1=Uniswap
DEX2=QuickSwap
MIN_PROFIT_THRESHOLD=1.0
```

## 5. Usage

### Run the Bot
```bash
cargo run
```

### Example Output
```
--- Price Update ---
Input: 1 WETH
Uniswap v3: 4470.63 USDC
QuickSwap v3: 4466.44 USDC
Simulated Profit: 3.21 USDC
```

## 6. Configuration

| Variable | Description | Example |
|----------|-------------|---------|
| `RPC_URL` | Polygon RPC endpoint | `https://polygon-rpc.com` |
| `TOKEN_PAIR` | Token pair to track | `WETH/USDC` |
| `MIN_PROFIT_THRESHOLD` | Minimum profit to consider (in USDC) | `1.0` |
| `DEX1`, `DEX2` | DEXes to compare | `Uniswap`, `QuickSwap` |

## 7. System Design / Architecture

The bot follows a simple yet effective architecture:

1. **RPC Connection**: Connects to Polygon RPC endpoint for blockchain data
2. **Price Fetching**: Retrieves token prices from DEX smart contracts
3. **Price Comparison**: Analyzes price differences between configured DEXes
4. **Profit Simulation**: Calculates potential arbitrage opportunities
5. **Logging**: Records profitable opportunities above the minimum threshold

### Architecture Flow
```
Polygon RPC ← Bot → DEX Contracts (Uniswap, QuickSwap)
     ↓
Price Analysis Engine
     ↓
Profit Calculator
     ↓
Logger/Alert System
```

### Key Components
- **Price Monitor**: Continuously fetches real-time prices
- **Arbitrage Calculator**: Simulates trade profitability
- **Configuration Manager**: Handles environment variables and settings
- **Logging System**: Outputs opportunities and system status





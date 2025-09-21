## Polygon arbitrage bot

#### A Rust bot that detects arbitrage opportunities across DEXes like Uniswap and QuickSwap on the Polygon network

### 1. Features
###### 1. Polls token prices of WETH/USDC from quotes.
###### 2. Detects arbitrage opportunities when price differences exceed a threshold.
###### 3. Simulates arbitrage profit including estimated gas cost which is 0.01$ per transaction.
###### 4. Configurable RPC URL, token pairs, and thresholds.
###### 5. Logs simulated opportunities to profit.txt file.


### 2. Installation & Setup

#### Prerequisites
- Rust (latest stable version)
- Polygon RPC endpoint (Alchemy, Infura, or public node), used public node for this projetc
- Cargo package manager

#### Clone the Repository
```bash
git clone https://github.com/dishachhabra11/polygon-arbitrage-bot
cd polygon-arbitrage-bot
```

#### Install Dependencies
```bash
cargo build
```

#### Setup Environment Variables
Create a `.env` file in the project root:

```env
POLYGON_RPC_URL=https://polygon-bor.publicnode.com
WETH=0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619
USDC=0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359
UNISWAP_QUOTER=0x61fFE014bA17989E743c5F6cB21bF9697530B21e
QUICKSWAP_QUOTER=0xa15F0D7377B2A0C0c10db057f641beD21028FC89
UNIV3_FEE=500
AMOUNT_IN_WETH=1.0
UNIV3_FEE=500           # 500 = 0.05% for USDC/WETH on Uni v3
START_USDC=10000        # starting trade in USDC
GAS_USDC_PER_TX=0.01    # estimated gas cost per transaction in USDC 0.01 (example)
PROFIT_THRESHOLD=0    # only alert if net profit > $0
```


#### Run the Bot
```bash
cargo run
```

### 3. System Design / Architecture

The bot follows a simple yet effective architecture:

1. **RPC Connection**: Connects to Polygon RPC endpoint for blockchain data
2. **Price Fetching**: Retrieves token prices from DEX smart contracts
3. **Price Comparison**: Analyzes price differences between configured DEXes
4. **Profit Simulation**: Calculates potential arbitrage opportunities
5. **Logging**: Records profitable opportunities above the minimum threshold

#### Architecture Flow
```
## ⚡ Polygon Arbitrage Bot Architecture

The bot detects arbitrage opportunities between **Uniswap v3** and **QuickSwap v3** on Polygon.

### 🔹 Components

- **Infrastructure**: Connects to Polygon via RPC (from `.env`).
- **Smart Contract Layer**:  
  - Uniswap v3 QuoterV2 (simulates swaps).  
  - QuickSwap v3 (Algebra) Quoter.  
- **Bot Logic**:  
  - Runs in a loop.  
  - Simulates two paths:
    1. Uni BUY (USDC→WETH) → Quick SELL (WETH→USDC)  
    2. Quick BUY (USDC→WETH) → Uni SELL (WETH→USDC)  
  - Calculates gross profit, subtracts gas, and picks best path.
- **Analytics**:  
  - Converts units, computes rates, and prints results.  
  - Logs profitable trades (`profit.txt`) if above threshold.

```






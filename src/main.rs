use anyhow::Result;
use dotenvy::dotenv;
use ethers::prelude::*;
use std::{env, sync::Arc, time::Duration};
use tokio::time::sleep;
use std::fs::OpenOptions;
use std::io::Write;

// ---- Uniswap v3 QuoterV2 ----
abigen!(
    UniswapQuoterV2,
    r#"[{
      "inputs": [{
        "components": [
          { "internalType": "address",  "name": "tokenIn",          "type": "address"  },
          { "internalType": "address",  "name": "tokenOut",         "type": "address"  },
          { "internalType": "uint256",  "name": "amountIn",         "type": "uint256"  },
          { "internalType": "uint24",   "name": "fee",              "type": "uint24"   },
          { "internalType": "uint160",  "name": "sqrtPriceLimitX96","type": "uint160"  }
        ],
        "internalType": "struct IQuoterV2.QuoteExactInputSingleParams",
        "name": "params",
        "type": "tuple"
      }],
      "name": "quoteExactInputSingle",
      "outputs": [
        { "internalType":"uint256","name":"amountOut","type":"uint256" },
        { "internalType":"uint160","name":"sqrtPriceX96After","type":"uint160" },
        { "internalType":"int24",  "name":"initializedTicksCrossed","type":"int24" },
        { "internalType":"uint256","name":"gasEstimate","type":"uint256" }
      ],
      "stateMutability": "view",
      "type": "function"
    }]"#
);

// ---- QuickSwap v3 (Algebra) Quoter ----
abigen!(
    AlgebraQuoter,
    r#"[{
      "inputs": [
        { "internalType": "address", "name": "tokenIn",        "type": "address" },
        { "internalType": "address", "name": "tokenOut",       "type": "address" },
        { "internalType": "uint256", "name": "amountIn",       "type": "uint256" },
        { "internalType": "uint160", "name": "limitSqrtPrice", "type": "uint160" }
      ],
      "name": "quoteExactInputSingle",
      "outputs": [
        { "internalType": "uint256", "name": "amountOut", "type": "uint256" }
      ],
      "stateMutability": "view",
      "type": "function"
    }]"#
);

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    // Provider
    let rpc_url = env::var("POLYGON_RPC_URL")?;
    let provider = Arc::new(Provider::<Http>::try_from(rpc_url)?.interval(Duration::from_millis(250)));

    // Sanity: live block
    let block = provider.get_block_number().await?;
    println!("Polygon latest block: {block}");

    // Addresses
    let weth: Address = env::var("WETH")?.parse()?;
    let usdc: Address = env::var("USDC")?.parse()?;
    let uni_quoter_addr: Address = env::var("UNISWAP_QUOTER")?.parse()?;
    let quick_quoter_addr: Address = env::var("QUICKSWAP_QUOTER")?.parse()?;

    // Contracts
    let uni_quoter = UniswapQuoterV2::new(uni_quoter_addr, provider.clone());
    let quick_quoter = AlgebraQuoter::new(quick_quoter_addr, provider.clone());

    // Config
    let fee: u32 = env::var("UNIV3_FEE")?.parse()?;  
    let start_usdc_f: f64 = env::var("START_USDC").unwrap_or("10000".into()).parse().unwrap_or(10000.0);
    let start_usdc = to_units(start_usdc_f, 6);      

    // Estimated gas in USDC per tx; assume 2 txs for round-trip
    let gas_usdc_per_tx_f: f64 = env::var("GAS_USDC_PER_TX").unwrap_or("0.0".into()).parse().unwrap_or(0.0);
    let gas_usdc_per_tx = to_units(gas_usdc_per_tx_f, 6);
    let round_trip_gas = gas_usdc_per_tx.checked_mul(U256::from(2)).unwrap_or_else(U256::zero);

    let profit_threshold_f: f64 = env::var("PROFIT_THRESHOLD").unwrap_or("0.0".into()).parse().unwrap_or(0.0);
    let profit_threshold = to_units(profit_threshold_f, 6);

    let zero_u160 = U256::zero(); 

    loop {
        // ---------- PATH A: Uni BUY (USDC->WETH) -> Quick SELL (WETH->USDC) ----------
        let path_a = async {
            // Buy WETH on Uni
            let buy_params = uniswap_quoter_v2::QuoteExactInputSingleParams {
                token_in: usdc,
                token_out: weth,
                amount_in: start_usdc,
                fee,
                sqrt_price_limit_x96: zero_u160,
            };
            let (weth_bought, _, _, _) = uni_quoter.quote_exact_input_single(buy_params).call().await?;
            // Sell WETH on Quick
            let usdc_back = quick_quoter.quote_exact_input_single(weth, usdc, weth_bought, zero_u160).call().await?;
            Ok::<(U256, U256), ContractError<Provider<Http>>>((weth_bought, usdc_back))
        }.await;

        // ---------- PATH B: Quick BUY (USDC->WETH) -> Uni SELL (WETH->USDC) ----------
        let path_b = async {
            // Buy WETH on Quick
            let weth_bought = quick_quoter.quote_exact_input_single(usdc, weth, start_usdc, zero_u160).call().await?;
            // Sell WETH on Uni
            let sell_params = uniswap_quoter_v2::QuoteExactInputSingleParams {
                token_in: weth,
                token_out: usdc,
                amount_in: weth_bought,
                fee,
                sqrt_price_limit_x96: zero_u160,
            };
            let (usdc_back, _, _, _) = uni_quoter.quote_exact_input_single(sell_params).call().await?;
            Ok::<(U256, U256), ContractError<Provider<Http>>>((weth_bought, usdc_back))
        }.await;

      
        let mut best_label = String::new();
        let mut best_weth = U256::zero();
        let mut best_back = U256::zero();
        let mut best_net = U256::zero();

        if let Ok((weth_a, back_a)) = path_a {
            let gross_a = back_a.saturating_sub(start_usdc);
            let net_a   = gross_a.saturating_sub(round_trip_gas);

            println!("\n--- PATH A: Uni BUY → Quick SELL ---");
            pretty_path(&start_usdc, &weth_a, &back_a, &round_trip_gas, &net_a, "Uni BUY", "Quick SELL");

            if best_label.is_empty() || net_a > best_net {
                best_label = "Uni BUY → Quick SELL".into();
                best_weth = weth_a;
                best_back = back_a;
                best_net = net_a;
            }
        } else {
            eprintln!("\n--- PATH A: Uni BUY → Quick SELL ---");
            eprintln!("Quote failed.");
        }

        if let Ok((weth_b, back_b)) = path_b {
            let gross_b = back_b.saturating_sub(start_usdc);
            let net_b   = gross_b.saturating_sub(round_trip_gas);

            println!("\n--- PATH B: Quick BUY → Uni SELL ---");
            pretty_path(&start_usdc, &weth_b, &back_b, &round_trip_gas, &net_b, "Quick BUY", "Uni SELL");

            if best_label.is_empty() || net_b > best_net {
                best_label = "Quick BUY → Uni SELL".into();
                best_weth = weth_b;
                best_back = back_b;
                best_net = net_b;
            }
        } else {
            eprintln!("\n--- PATH B: Quick BUY → Uni SELL ---");
            eprintln!("Quote failed.");
        }

        // Show chosen path and decide if we log
        if best_label.is_empty() {
            eprintln!("\nBoth paths failed to quote this round.");
        } else {
            let start_usdc_s = fmt_units(start_usdc, 6);
            let weth_s = fmt_units(best_weth, 18);
            let back_s = fmt_units(best_back, 6);
            let gas_s = fmt_units(round_trip_gas, 6);
            let net_s = fmt_units(best_net, 6);

            println!("\n=== Best Path Selected: {} ===", best_label);
            println!("Start: {} USDC | WETH bought: {} | USDC back: {} | Gas: {} | Net: {}", start_usdc_s, weth_s, back_s, gas_s, net_s);

            if best_net > profit_threshold {
                println!("  🚀🚀 ARB DETECTED ({}): {} USDC", best_label, net_s);
                let log_entry = format!(
                    "ARB ({label}): net={net} USDC | start={start} | weth_bought={weth} | usdc_back={back} | gas={gas}\n",
                    label = best_label,
                    net = net_s,
                    start = start_usdc_s,
                    weth = weth_s,
                    back = back_s,
                    gas = gas_s
                );
                append_to_file("profit.txt", &log_entry);
            } else {
                println!("No arbitrage (net ≤ threshold).");
            }
        }

        sleep(Duration::from_secs(5)).await;
    }
}

fn to_units(amount: f64, decimals: u32) -> U256 {
    let scale = 10u128.pow(decimals);
    U256::from((amount * scale as f64).round() as u128)
}

fn fmt_units(amount: U256, decimals: u32) -> String {
    if decimals == 0 { return amount.to_string(); }
    let ten = U256::from(10);
    let scale = ten.pow(U256::from(decimals));
    let int = amount / scale;
    let mut frac = (amount % scale).to_string();
    while frac.len() < decimals as usize { frac.insert(0, '0'); }
    while frac.ends_with('0') { frac.pop(); }
    if frac.is_empty() { int.to_string() } else { format!("{}.{}", int, frac) }
}

fn append_to_file(path: &str, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        if let Err(e) = file.write_all(line.as_bytes()) {
            eprintln!("Failed to write to {path}: {e:?}");
        }
    } else {
        eprintln!("Failed to open {path}");
    }
}

fn pretty_path(start_usdc: &U256, weth_bought: &U256, usdc_back: &U256, gas_usdc: &U256, net_profit: &U256, buy_tag: &str, sell_tag: &str) {
    let start_usdc_s = fmt_units(*start_usdc, 6);
    let weth_s = fmt_units(*weth_bought, 18);
    let back_s = fmt_units(*usdc_back, 6);
    let gas_s = fmt_units(*gas_usdc, 6);
    let net_s = fmt_units(*net_profit, 6);

    // Implied rates (for display only)
    let buy_rate_weth_per_usdc = safe_ratio(*weth_bought, *start_usdc, 18, 6); // WETH per 1 USDC
    let sell_rate_usdc_per_weth = safe_ratio(*usdc_back, *weth_bought, 6, 18); // USDC per 1 WETH

    println!("Start: {} USDC", start_usdc_s);
    println!("{}: {} WETH (≈ {} WETH/USDC)", buy_tag, weth_s, buy_rate_weth_per_usdc);
    println!("{}: {} USDC (≈ {} USDC/WETH)", sell_tag, back_s, sell_rate_usdc_per_weth);
    println!("Gross diff: {} USDC", fmt_units(usdc_back.saturating_sub(*start_usdc), 6));
    println!("Est. gas (round-trip): {} USDC", gas_s);
    println!("Net Profit: {} USDC", net_s);
}

fn safe_ratio(num: U256, den: U256, num_decimals: u32, den_decimals: u32) -> String {
    if den.is_zero() { return "NA".to_string(); }
    let target_dp = 18u32;
    let mut scale_pow: i32 = (target_dp as i32) + (den_decimals as i32) - (num_decimals as i32);
    let mut scale = U256::one();
    if scale_pow < 0 {
        for _ in 0..(-scale_pow) { scale = scale / U256::from(10u8); }
    } else {
        for _ in 0..scale_pow { scale = scale.saturating_mul(U256::from(10u8)); }
    }
    let num_scaled = num.saturating_mul(scale);
    let q = num_scaled / den;
    // print q as 18-dp decimal
    fmt_units(q, target_dp)
}

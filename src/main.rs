use anyhow::Result;
use dotenvy::dotenv;
use ethers::prelude::*;
use std::{env, sync::Arc, time::Duration};
use tokio::time::sleep;
use std::fs::OpenOptions;
use std::io::Write;

// ---- Uniswap v3 QuoterV2 (JSON ABI because of tuple param) ----
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
    let provider = Arc::new(
        Provider::<Http>::try_from(rpc_url)?
            .interval(Duration::from_millis(250))
    );

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
    let fee: u32 = env::var("UNIV3_FEE")?.parse()?;              // Uni v3 fee tier (uint24)
    let start_usdc_f: f64 = env::var("START_USDC").unwrap_or("10000".into()).parse().unwrap_or(10000.0);
    let start_usdc = to_units(start_usdc_f, 6);                   // USDC 6 decimals

    // Gas estimate (USDC). For round trip assume 2 tx (router/approvals excluded here)
    let gas_usdc_per_tx_f: f64 = env::var("GAS_USDC_PER_TX").unwrap_or("0.02".into()).parse().unwrap_or(0.02);
    let gas_usdc_per_tx = to_units(gas_usdc_per_tx_f, 6);
    let round_trip_gas = gas_usdc_per_tx.checked_mul(U256::from(2)).unwrap_or_else(U256::zero);

    // Profit threshold (USDC)
    let profit_threshold_f: f64 = env::var("PROFIT_THRESHOLD").unwrap_or("0.1".into()).parse().unwrap_or(0.1);
    let profit_threshold = to_units(profit_threshold_f, 6);

    let zero_u160 = U256::zero(); // no sqrt price limit

    loop {
        // ---------- PATH A: Uni BUY (USDC->WETH) -> Quick SELL (WETH->USDC) ----------
        let mut a_ok = false;
        let mut weth_a = U256::zero();
        let mut back_a = U256::zero();

        // Buy WETH on Uni
        let buy_params_a = uniswap_quoter_v2::QuoteExactInputSingleParams {
            token_in: usdc,
            token_out: weth,
            amount_in: start_usdc,
            fee,
            sqrt_price_limit_x96: zero_u160,
        };
        match uni_quoter.quote_exact_input_single(buy_params_a).call().await {
            Ok((weth_out, _, _, _)) => {
                // Sell WETH on Quick
                match quick_quoter.quote_exact_input_single(weth, usdc, weth_out, zero_u160).call().await {
                    Ok(usdc_back) => { a_ok = true; weth_a = weth_out; back_a = usdc_back; }
                    Err(e) => eprintln!("QuickSwap quote error (A WETH->USDC): {e:?}"),
                }
            }
            Err(e) => eprintln!("Uniswap quote error (A USDC->WETH): {e:?}"),
        }

        // ---------- PATH B: Quick BUY (USDC->WETH) -> Uni SELL (WETH->USDC) ----------
        let mut b_ok = false;
        let mut weth_b = U256::zero();
        let mut back_b = U256::zero();

        match quick_quoter.quote_exact_input_single(usdc, weth, start_usdc, zero_u160).call().await {
            Ok(weth_out) => {
                let sell_params_b = uniswap_quoter_v2::QuoteExactInputSingleParams {
                    token_in: weth,
                    token_out: usdc,
                    amount_in: weth_out,
                    fee,
                    sqrt_price_limit_x96: zero_u160,
                };
                match uni_quoter.quote_exact_input_single(sell_params_b).call().await {
                    Ok((usdc_back, _, _, _)) => { b_ok = true; weth_b = weth_out; back_b = usdc_back; }
                    Err(e) => eprintln!("Uniswap quote error (B WETH->USDC): {e:?}"),
                }
            }
            Err(e) => eprintln!("QuickSwap quote error (B USDC->WETH): {e:?}"),
        }

        // ----- Print both paths with signed diffs -----
        if a_ok {
            println!("\n--- PATH A: Uni BUY → Quick SELL ---");
            pretty_path(start_usdc, weth_a, back_a, round_trip_gas, "Uni BUY", "Quick SELL");
        } else {
            eprintln!("\n--- PATH A: Uni BUY → Quick SELL ---");
            eprintln!("Quote failed.");
        }

        if b_ok {
            println!("\n--- PATH B: Quick BUY → Uni SELL ---");
            pretty_path(start_usdc, weth_b, back_b, round_trip_gas, "Quick BUY", "Uni SELL");
        } else {
            eprintln!("\n--- PATH B: Quick BUY → Uni SELL ---");
            eprintln!("Quote failed.");
        }

        // ----- Choose the better path (highest USDC back) -----
        let (best_label, best_weth, best_back, best_ok) = match (a_ok, b_ok) {
            (true, true) => {
                if back_a > back_b {
                    ("Uni BUY → Quick SELL", weth_a, back_a, true)
                } else {
                    ("Quick BUY → Uni SELL", weth_b, back_b, true)
                }
            }
            (true, false) => ("Uni BUY → Quick SELL", weth_a, back_a, true),
            (false, true) => ("Quick BUY → Uni SELL", weth_b, back_b, true),
            (false, false) => ("", U256::zero(), U256::zero(), false),
        };

        if !best_ok {
            eprintln!("\nBoth paths failed to quote this round.");
            sleep(Duration::from_secs(5)).await;
            continue;
        }

        // Signed net = back - start - gas
        let net_i128 = signed_diff(best_back, start_usdc, round_trip_gas);
        let net_abs_u256 = if net_i128 >= 0 { U256::from(net_i128 as u128) } else { U256::from((-net_i128) as u128) };
        let net_str = if net_i128 >= 0 {
            fmt_units(net_abs_u256, 6)
        } else {
            format!("-{}", fmt_units(net_abs_u256, 6))
        };

        println!("\n=== Best Path Selected: {} ===", best_label);
        println!(
            "Start: {} USDC | WETH bought: {} | USDC back: {} | Gas: {} | Net: {}",
            fmt_units(start_usdc, 6),
            fmt_units(best_weth, 18),
            fmt_units(best_back, 6),
            fmt_units(round_trip_gas, 6),
            net_str
        );

        // Threshold check using signed math
        let thresh_i128 = profit_threshold.as_u128() as i128;
        if net_i128 > thresh_i128 {
            println!("  🚀🚀 ARB DETECTED ({}): {} USDC", best_label, net_str);
            let log_entry = format!(
                "ARB ({label}): net={net} USDC | start={start} USDC | weth_bought={weth} | usdc_back={back} | gas={gas}\n",
                label = best_label,
                net = net_str,
                start = fmt_units(start_usdc,6),
                weth = fmt_units(best_weth,18),
                back = fmt_units(best_back,6),
                gas = fmt_units(round_trip_gas,6)
            );
            append_to_file("profit.txt", &log_entry);
        } else {
            println!("No arbitrage (net ≤ threshold).");
        }

        sleep(Duration::from_secs(5)).await;
    }
}

// ---------------- helpers ----------------

fn to_units(amount: f64, decimals: u32) -> U256 {
    // For config-like inputs; for production, prefer integer math end-to-end.
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

/// Signed profit in USDC (i128) = back - start - gas (all USDC, 6dp)
fn signed_diff(back: U256, start: U256, gas: U256) -> i128 {
    let b = back.as_u128() as i128;
    let s = start.as_u128() as i128;
    let g = gas.as_u128() as i128;
    b - s - g
}

/// Pretty print a path with implied rates and **signed** diffs
fn pretty_path(start_usdc: U256, weth_bought: U256, usdc_back: U256, gas_usdc: U256, buy_tag: &str, sell_tag: &str) {
    let buy_rate_weth_per_usdc = ratio_string(weth_bought, start_usdc, 18, 6); // WETH per 1 USDC
    let sell_rate_usdc_per_weth = ratio_string(usdc_back, weth_bought, 6, 18); // USDC per 1 WETH

    // gross signed (no gas) and net signed (with gas)
    let gross_i128 = signed_diff(usdc_back, start_usdc, U256::zero());
    let net_i128   = signed_diff(usdc_back, start_usdc, gas_usdc);

    let gross_abs = if gross_i128 >= 0 { U256::from(gross_i128 as u128) } else { U256::from((-gross_i128) as u128) };
    let net_abs   = if net_i128 >= 0 { U256::from(net_i128 as u128) } else { U256::from((-net_i128) as u128) };

    let gross_str = if gross_i128 >= 0 { fmt_units(gross_abs, 6) } else { format!("-{}", fmt_units(gross_abs, 6)) };
    let net_str   = if net_i128   >= 0 { fmt_units(net_abs,   6) } else { format!("-{}", fmt_units(net_abs,   6)) };

    println!("Start: {} USDC", fmt_units(start_usdc, 6));
    println!("{}: {} WETH (≈ {} WETH/USDC)", buy_tag, fmt_units(weth_bought, 18), buy_rate_weth_per_usdc);
    println!("{}: {} USDC (≈ {} USDC/WETH)", sell_tag, fmt_units(usdc_back, 6),  sell_rate_usdc_per_weth);
    println!("Gross diff: {} USDC", gross_str);
    println!("Est. gas (round-trip): {} USDC", fmt_units(gas_usdc, 6));
    println!("Net Profit: {} USDC", net_str);
}

/// Safe ratio as a string: (num / den) with decimals, scaled to 18dp for display
fn ratio_string(num: U256, den: U256, num_decimals: u32, den_decimals: u32) -> String {
    if den.is_zero() { return "NA".to_string(); }

    // target 18 dp: (num * 10^(18 + den_dec - num_dec)) / den
    let target_dp = 18u32;
    let exp = (target_dp as i32) + (den_decimals as i32) - (num_decimals as i32);
    let mut scale = U256::one();

    if exp >= 0 {
        for _ in 0..(exp as u32) { scale = scale.saturating_mul(U256::from(10u8)); }
    } else {
        // If negative, we would downscale; to keep integers, we instead
        // reduce precision by not upscaling fully (fine for display)
        // Here we just avoid scaling further (rare for these tokens).
    }

    let num_scaled = num.saturating_mul(scale);
    let q = num_scaled / den;
    fmt_units(q, target_dp)
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

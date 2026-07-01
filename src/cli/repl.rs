use std::io::{self, Write};

use rust_decimal::Decimal;

use aegis_oms::service::order_service::OrderService;

use super::handlers::{
    handle_account, handle_cancel, handle_deposit, handle_fill, handle_history, handle_list,
    handle_margin, handle_positions, handle_submit, handle_withdraw, parse_mark_prices,
};

pub fn run_repl(svc: &mut OrderService) {
    println!("Aegis OMS REPL (type 'help' for commands, 'quit' to exit)");
    loop {
        print!("aegis> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() || input.is_empty() {
            break;
        }
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "help" | "h" => print_help(),
            "submit" | "s" if parts.len() >= 6 => {
                let price: Decimal = match parts[4].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid price '{}'. Expected a decimal number.",
                            parts[4]
                        );
                        continue;
                    }
                };
                let qty: Decimal = match parts[5].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid quantity '{}'. Expected a decimal number.",
                            parts[5]
                        );
                        continue;
                    }
                };
                handle_submit(svc, parts[1], parts[2], parts[3], price, qty);
            }
            "list" | "l" => handle_list(svc),
            "cancel" | "c" if parts.len() >= 2 => handle_cancel(svc, parts[1]),
            "fill" | "f" if parts.len() >= 4 => {
                let qty: Decimal = match parts[2].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid quantity '{}'. Expected a decimal number.",
                            parts[2]
                        );
                        continue;
                    }
                };
                let price: Decimal = match parts[3].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid price '{}'. Expected a decimal number.",
                            parts[3]
                        );
                        continue;
                    }
                };
                handle_fill(svc, parts[1], qty, price);
            }
            "account" | "a" => handle_account(svc),
            "positions" | "p" => handle_positions(svc),
            "history" => {
                let instrument = parts.get(1).copied();
                handle_history(svc, instrument);
            }
            "margin" | "m" => {
                let price_args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
                let mark_prices = parse_mark_prices(&price_args);
                handle_margin(svc, &mark_prices);
            }
            "deposit" if parts.len() >= 2 => {
                let amount: Decimal = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid amount '{}'. Expected a decimal number.",
                            parts[1]
                        );
                        continue;
                    }
                };
                handle_deposit(svc, amount);
            }
            "withdraw" if parts.len() >= 2 => {
                let amount: Decimal = match parts[1].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        eprintln!(
                            "Error: Invalid amount '{}'. Expected a decimal number.",
                            parts[1]
                        );
                        continue;
                    }
                };
                handle_withdraw(svc, amount);
            }
            "quit" | "q" | "exit" => break,
            _ => eprintln!("Unknown command. Type 'help' for usage."),
        }
    }
}

fn print_help() {
    println!("Commands:");
    println!("  submit <instrument> <buy|sell> <limit|market> <price> <qty>");
    println!("  list                       - List open orders");
    println!("  cancel <order_id>          - Cancel an order");
    println!("  fill <order_id> <qty> <price> - Simulate a fill");
    println!("  account                    - Show account balance");
    println!("  positions                  - Show open positions");
    println!("  history [instrument]       - Show trade history");
    println!("  margin [SYMBOL:PRICE ...]  - Show margin status");
    println!("  deposit <amount>           - Deposit funds");
    println!("  withdraw <amount>          - Withdraw funds");
    println!("  quit                       - Exit REPL");
}

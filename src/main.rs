use std::io::{self, Write};

use aegis_oms::domain::account::Account;
use aegis_oms::domain::instrument::{AssetClass, Instrument};
use aegis_oms::domain::order::*;
use aegis_oms::infra::event_bus::EventBus;
use aegis_oms::service::order_service::{NewOrderRequest, OrderService};
use aegis_oms::service::risk_check::{RiskChecker, RiskLimits};
use clap::{Parser, Subcommand};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Parser)]
#[command(name = "aegis-oms", about = "Order Management System for FX and Crypto")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit a new order
    Submit {
        #[arg(short, long)]
        instrument: String,
        #[arg(short, long)]
        side: String,
        #[arg(short = 't', long, default_value = "limit")]
        r#type: String,
        #[arg(short, long, default_value = "0")]
        price: Decimal,
        #[arg(short, long)]
        qty: Decimal,
    },
    /// List open orders
    List,
    /// Cancel an order
    Cancel {
        /// Order ID
        id: String,
    },
    /// Show account balance
    Account,
    /// Show open positions
    Positions,
    /// Simulate a fill (for testing)
    Fill {
        /// Order ID
        id: String,
        #[arg(short, long)]
        qty: Decimal,
        #[arg(short, long)]
        price: Decimal,
    },
    /// Interactive REPL mode
    Repl,
}

fn default_instruments() -> Vec<Instrument> {
    vec![
        Instrument {
            symbol: "USD/JPY".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        },
        Instrument {
            symbol: "EUR/USD".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.00001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        },
        Instrument {
            symbol: "BTC/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            leverage: dec!(2),
        },
        Instrument {
            symbol: "ETH/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            leverage: dec!(2),
        },
    ]
}

fn create_service() -> OrderService {
    let account = Account::new("acc-001", "Default", dec!(100000));
    let instruments = default_instruments();
    let risk = RiskChecker::new(RiskLimits::default());
    let bus = EventBus::new();
    OrderService::new(account, instruments, risk, bus)
}

fn parse_side(s: &str) -> Result<Side, String> {
    match s.to_lowercase().as_str() {
        "buy" | "b" => Ok(Side::Buy),
        "sell" | "s" => Ok(Side::Sell),
        _ => Err(format!("Invalid side: {s}. Use 'buy' or 'sell'")),
    }
}

fn parse_order_type(type_str: &str, price: Decimal) -> OrderType {
    match type_str.to_lowercase().as_str() {
        "market" | "m" => OrderType::Market,
        _ => OrderType::Limit { price },
    }
}

fn handle_submit(
    svc: &mut OrderService,
    instrument: &str,
    side_str: &str,
    type_str: &str,
    price: Decimal,
    qty: Decimal,
) {
    let side = match parse_side(side_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };
    let order_type = parse_order_type(type_str, price);

    match svc.submit_order(NewOrderRequest {
        instrument: instrument.into(),
        side,
        order_type,
        quantity: qty,
        time_in_force: TimeInForce::GTC,
    }) {
        Ok(id) => println!("Order created: {id}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn handle_list(svc: &OrderService) {
    let orders = svc.get_open_orders();
    if orders.is_empty() {
        println!("No open orders.");
        return;
    }
    println!(
        "{:<28} {:<10} {:<5} {:<12} {:<10} {:<8} STATUS",
        "ID", "INSTRUMENT", "SIDE", "TYPE", "QTY", "FILLED"
    );
    for o in orders {
        println!(
            "{:<28} {:<10} {:<5} {:<12} {:<10} {:<8} {}",
            o.id, o.instrument, o.side, o.order_type, o.quantity, o.filled_quantity, o.status,
        );
    }
}

fn handle_account(svc: &OrderService) {
    let acc = svc.get_account();
    println!("{:<28} {:<10} {:<14} {:<14} AVAILABLE", "ID", "NAME", "BALANCE", "LOCKED");
    println!(
        "{:<28} {:<10} {:<14} {:<14} {}",
        acc.id,
        acc.name,
        acc.balance,
        acc.locked_margin,
        acc.available_balance()
    );
}

fn handle_positions(svc: &OrderService) {
    let positions = svc.get_positions();
    if positions.is_empty() {
        println!("No open positions.");
        return;
    }
    println!("{:<10} {:<5} {:<10} {:<12} UNREALIZED_PNL", "INSTRUMENT", "SIDE", "QTY", "AVG_PRICE");
    for p in positions {
        println!(
            "{:<10} {:<5} {:<10} {:<12} {}",
            p.instrument, p.side, p.quantity, p.avg_price, p.unrealized_pnl,
        );
    }
}

fn handle_cancel(svc: &mut OrderService, id_str: &str) {
    let id = match ulid::Ulid::from_string(id_str) {
        Ok(ulid) => OrderId(ulid),
        Err(e) => {
            eprintln!("Invalid order ID: {e}");
            return;
        }
    };
    match svc.cancel_order(&id) {
        Ok(()) => println!("Order cancelled: {id}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn handle_fill(svc: &mut OrderService, id_str: &str, qty: Decimal, price: Decimal) {
    let id = match ulid::Ulid::from_string(id_str) {
        Ok(ulid) => OrderId(ulid),
        Err(e) => {
            eprintln!("Invalid order ID: {e}");
            return;
        }
    };
    match svc.fill_order(&id, qty, price) {
        Ok(()) => println!("Order filled: {id} qty={qty} price={price}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn run_repl(svc: &mut OrderService) {
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
            "help" | "h" => {
                println!("Commands:");
                println!("  submit <instrument> <buy|sell> <limit|market> <price> <qty>");
                println!("  list                  - List open orders");
                println!("  cancel <order_id>     - Cancel an order");
                println!("  fill <order_id> <qty> <price> - Simulate a fill");
                println!("  account               - Show account balance");
                println!("  positions             - Show open positions");
                println!("  quit                  - Exit REPL");
            }
            "submit" | "s" if parts.len() >= 6 => {
                let price: Decimal = parts[4].parse().unwrap_or_default();
                let qty: Decimal = parts[5].parse().unwrap_or_default();
                handle_submit(svc, parts[1], parts[2], parts[3], price, qty);
            }
            "list" | "l" => handle_list(svc),
            "cancel" | "c" if parts.len() >= 2 => handle_cancel(svc, parts[1]),
            "fill" | "f" if parts.len() >= 4 => {
                let qty: Decimal = parts[2].parse().unwrap_or_default();
                let price: Decimal = parts[3].parse().unwrap_or_default();
                handle_fill(svc, parts[1], qty, price);
            }
            "account" | "a" => handle_account(svc),
            "positions" | "p" => handle_positions(svc),
            "quit" | "q" | "exit" => break,
            _ => eprintln!("Unknown command. Type 'help' for usage."),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let mut svc = create_service();

    match cli.command {
        Some(Commands::Submit { instrument, side, r#type, price, qty }) => {
            handle_submit(&mut svc, &instrument, &side, &r#type, price, qty)
        }
        Some(Commands::List) => handle_list(&svc),
        Some(Commands::Cancel { id }) => handle_cancel(&mut svc, &id),
        Some(Commands::Fill { id, qty, price }) => handle_fill(&mut svc, &id, qty, price),
        Some(Commands::Account) => handle_account(&svc),
        Some(Commands::Positions) => handle_positions(&svc),
        Some(Commands::Repl) | None => run_repl(&mut svc),
    }
}

use std::collections::HashMap;

use rust_decimal::Decimal;

use aegis_oms::domain::order::{OrderId, OrderType, Side, TimeInForce};
use aegis_oms::domain::risk_engine::{MarginLevel, MarginStatus};
use aegis_oms::service::order_service::{NewOrderRequest, OrderService};

pub fn parse_side(s: &str) -> Result<Side, String> {
    match s.to_lowercase().as_str() {
        "buy" | "b" => Ok(Side::Buy),
        "sell" | "s" => Ok(Side::Sell),
        _ => Err(format!("Invalid side: {s}. Use 'buy' or 'sell'")),
    }
}

pub fn parse_order_type(type_str: &str, price: Decimal) -> OrderType {
    match type_str.to_lowercase().as_str() {
        "market" | "m" => OrderType::Market,
        _ => OrderType::Limit { price },
    }
}

pub fn parse_mark_prices(prices: &[String]) -> HashMap<String, Decimal> {
    prices
        .iter()
        .filter_map(|s| {
            let mut parts = s.splitn(2, ':');
            let sym = parts.next()?.to_string();
            let price: Decimal = parts.next()?.parse().ok()?;
            Some((sym, price))
        })
        .collect()
}

pub fn parse_order_id(id_str: &str) -> Result<OrderId, String> {
    ulid::Ulid::from_string(id_str).map(OrderId).map_err(|e| format!("Invalid order ID: {e}"))
}

pub fn handle_submit(
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

pub fn handle_list(svc: &OrderService) {
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

pub fn handle_account(svc: &OrderService) {
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

pub fn handle_positions(svc: &OrderService) {
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

pub fn handle_history(svc: &OrderService, instrument: Option<&str>) {
    let trades = svc.get_trade_history(instrument);
    if trades.is_empty() {
        println!("No trade history.");
        return;
    }
    println!("{:<28} {:<10} {:<5} {:<10} {:<12} PNL", "ID", "INSTRUMENT", "SIDE", "QTY", "PRICE");
    for t in trades {
        println!(
            "{:<28} {:<10} {:<5} {:<10} {:<12} {}",
            t.id,
            t.instrument,
            t.side,
            t.quantity,
            t.price,
            t.realized_pnl.map_or("-".into(), |p| p.to_string()),
        );
    }
}

pub fn handle_margin(svc: &OrderService, mark_prices: &HashMap<String, Decimal>) {
    let leverages: HashMap<String, Decimal> =
        svc.instruments.iter().map(|(sym, inst)| (sym.clone(), inst.leverage)).collect();

    let positions: Vec<_> = svc.get_positions();
    let status = MarginStatus::calculate(
        svc.get_account(),
        &positions,
        mark_prices,
        &leverages,
        Decimal::new(100, 0),
        Decimal::new(50, 0),
    );

    let level_str = match status.level {
        MarginLevel::Normal => "Normal",
        MarginLevel::MarginCall => "⚠ MarginCall",
        MarginLevel::StopOut => "🚨 StopOut",
    };

    println!(
        "{:<20} {:<20} {:<20} {:<20} {:<20} LEVEL",
        "EQUITY", "USED_MARGIN", "FREE_MARGIN", "MARGIN_RATIO%", "EFF_LEVERAGE"
    );
    println!(
        "{:<20} {:<20} {:<20} {:<20} {:<20} {}",
        status.equity,
        status.used_margin,
        status.free_margin,
        format!("{:.2}", status.margin_ratio),
        format!("{:.2}", status.effective_leverage),
        level_str,
    );
}

pub fn handle_cancel(svc: &mut OrderService, id_str: &str) {
    let id = match parse_order_id(id_str) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };
    match svc.cancel_order(&id) {
        Ok(()) => println!("Order cancelled: {id}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

pub fn handle_fill(svc: &mut OrderService, id_str: &str, qty: Decimal, price: Decimal) {
    let id = match parse_order_id(id_str) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };
    match svc.fill_order(&id, qty, price) {
        Ok(()) => println!("Order filled: {id} qty={qty} price={price}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}

pub fn handle_deposit(svc: &mut OrderService, amount: Decimal) {
    match svc.deposit(amount) {
        Ok(()) => println!("Deposited: {amount}. New balance: {}", svc.get_account().balance),
        Err(e) => eprintln!("Error: {e}"),
    }
}

pub fn handle_withdraw(svc: &mut OrderService, amount: Decimal) {
    match svc.withdraw(amount) {
        Ok(()) => println!("Withdrawn: {amount}. New balance: {}", svc.get_account().balance),
        Err(e) => eprintln!("Error: {e}"),
    }
}

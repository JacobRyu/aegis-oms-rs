use aegis_oms::domain::account::Account;
use aegis_oms::domain::instrument::{AssetClass, Instrument};
use aegis_oms::domain::order::*;
use aegis_oms::infra::event_bus::EventBus;
use aegis_oms::service::order_service::{NewOrderRequest, OrderService};
use aegis_oms::service::risk_check::{RiskChecker, RiskLimits};
use rust_decimal_macros::dec;

fn main() {
    println!("=== Testing Position Reversal ===\n");

    let account = Account::new("acc-001", "Test", dec!(100000));
    let instruments = vec![Instrument {
        symbol: "BTC/USD".into(),
        asset_class: AssetClass::Crypto,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        leverage: dec!(2),
    }];
    let risk = RiskChecker::new(RiskLimits::default());
    let bus = EventBus::new();
    let mut svc = OrderService::new(account, instruments, risk, bus);

    // Open long position: buy 1 BTC at 65000
    println!("Opening long position: buy 1 BTC");
    let id1 = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: OrderType::Limit { price: dec!(65000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();
    svc.fill_order(&id1, dec!(1.0), dec!(65000)).unwrap();

    let positions = svc.get_positions();
    println!(
        "Position after buy: {} {} @ {}",
        positions[0].side, positions[0].quantity, positions[0].avg_price
    );

    // Now sell 2 BTC (should close 1 BTC long and open 1 BTC short)
    println!("\nSelling 2 BTC (more than position)");
    let id2 = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Sell,
            order_type: OrderType::Limit { price: dec!(67000) },
            quantity: dec!(2.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();
    svc.fill_order(&id2, dec!(2.0), dec!(67000)).unwrap();

    let positions = svc.get_positions();
    if positions.is_empty() {
        println!("WARNING: Position is flat. Expected 1 BTC short position!");
    } else {
        println!(
            "Position after sell: {} {} @ {}",
            positions[0].side, positions[0].quantity, positions[0].avg_price
        );
    }
}

mod common;

use aegis_oms::domain::order::{OrderStatus, Side, TimeInForce};
use aegis_oms::service::composite_order_service::CompositeOrderService;
use aegis_oms::service::order_service::NewOrderRequest;
use rust_decimal_macros::dec;

#[test]
fn stop_buy_triggers_when_price_reaches_trigger_level() {
    let mut svc = common::setup_service();
    let mut composite = CompositeOrderService::new();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Stop { trigger_price: dec!(70000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let triggered = composite.on_price_update(&mut svc, "BTC/USD", dec!(70000)).unwrap();

    assert_eq!(triggered.len(), 1);
    assert_eq!(triggered[0], id);

    svc.fill_order(&id, dec!(1.0), dec!(70000)).unwrap();
    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Filled);
}

#[test]
fn stop_limit_triggers_and_converts_to_limit() {
    let mut svc = common::setup_service();
    let mut composite = CompositeOrderService::new();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::StopLimit {
                trigger_price: dec!(68000),
                limit_price: dec!(68100),
            },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let triggered = composite.on_price_update(&mut svc, "BTC/USD", dec!(68000)).unwrap();

    assert_eq!(triggered.len(), 1);
    assert_eq!(triggered[0], id);

    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Accepted);
}

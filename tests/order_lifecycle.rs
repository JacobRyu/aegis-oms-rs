mod common;

use aegis_oms::domain::order::{OrderStatus, Side, TimeInForce};
use aegis_oms::service::order_service::NewOrderRequest;
use rust_decimal_macros::dec;

#[test]
fn full_limit_order_lifecycle() {
    let mut svc = common::setup_service();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(65000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Accepted);

    let fill_price = dec!(65000);
    svc.fill_order(&id, dec!(1.0), fill_price).unwrap();

    let filled = svc.get_order(&id).unwrap();
    assert_eq!(filled.status, OrderStatus::Filled);

    let positions = svc.get_positions();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, dec!(1.0));
}

#[test]
fn cancel_accepted_order() {
    let mut svc = common::setup_service();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(60000) },
            quantity: dec!(0.5),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    svc.cancel_order(&id).unwrap();
    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
}

#[test]
fn partial_fill_then_close() {
    let mut svc = common::setup_service();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Market,
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    svc.fill_order(&id, dec!(0.4), dec!(65000)).unwrap();
    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_quantity, dec!(0.4));

    svc.fill_order(&id, dec!(0.6), dec!(65100)).unwrap();
    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Filled);

    let positions = svc.get_positions();
    assert_eq!(positions[0].quantity, dec!(1.0));
    assert_eq!(positions[0].avg_price, dec!(65060));
}

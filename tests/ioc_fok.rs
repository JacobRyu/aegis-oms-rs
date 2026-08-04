mod common;

use aegis_oms::domain::order::{OrderStatus, Side, TimeInForce};
use aegis_oms::service::order_service::NewOrderRequest;
use rust_decimal_macros::dec;

#[test]
fn ioc_order_auto_cancelled_after_submit() {
    let mut svc = common::setup_service();

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Market,
            quantity: dec!(1.0),
            time_in_force: TimeInForce::IOC,
        })
        .unwrap();

    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
}

#[test]
fn fok_rejected_at_submission() {
    let mut svc = common::setup_service();

    let result = svc.submit_order(NewOrderRequest {
        instrument: "BTC/USD".into(),
        side: Side::Buy,
        order_type: aegis_oms::domain::order::OrderType::Market,
        quantity: dec!(5.0),
        time_in_force: TimeInForce::FOK,
    });

    assert!(result.is_err());
}

#[test]
fn gtc_order_not_affected_by_ioc_fok() {
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

    let order = svc.get_order(&id).unwrap();
    assert_eq!(order.status, OrderStatus::Accepted);
}

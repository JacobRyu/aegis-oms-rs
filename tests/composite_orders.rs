mod common;

use aegis_oms::domain::composite_order::{IfdOrder, OcoOrder};
use aegis_oms::domain::order::{OrderStatus, Side, TimeInForce};
use aegis_oms::service::composite_order_service::CompositeOrderService;
use aegis_oms::service::order_service::NewOrderRequest;
use rust_decimal_macros::dec;

#[test]
fn oco_cancels_other_on_fill() {
    let mut svc = common::setup_service();
    let mut composite = CompositeOrderService::new();

    let id_a = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(60000) },
            quantity: dec!(0.5),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let id_b = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(70000) },
            quantity: dec!(0.5),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let oco = OcoOrder::new(id_a, id_b);
    composite.register_oco(oco);

    svc.fill_order(&id_a, dec!(0.5), dec!(60000)).unwrap();

    composite.on_order_filled(id_a, &mut svc).unwrap();

    let order_b = svc.get_order(&id_b).unwrap();
    assert_eq!(order_b.status, OrderStatus::Cancelled);
}

#[test]
fn ifd_secondary_submitted_after_primary_fill() {
    let mut svc = common::setup_service();
    let mut composite = CompositeOrderService::new();

    let primary = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Market,
            quantity: dec!(0.3),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let secondary = NewOrderRequest {
        instrument: "BTC/USD".into(),
        side: Side::Sell,
        order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(70000) },
        quantity: dec!(0.3),
        time_in_force: TimeInForce::GTC,
    };

    let ifd = IfdOrder::new(primary, secondary);
    composite.register_ifd(ifd);

    svc.fill_order(&primary, dec!(0.3), dec!(65000)).unwrap();

    composite.on_order_filled(primary, &mut svc).unwrap();

    let open_orders = svc.get_open_orders();
    let secondary =
        open_orders.iter().find(|o| o.side == Side::Sell).expect("secondary sell order");
    assert_eq!(secondary.status, OrderStatus::Accepted);
    assert_eq!(secondary.instrument, "BTC/USD");
}

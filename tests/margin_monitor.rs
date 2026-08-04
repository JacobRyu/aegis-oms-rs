mod common;

use aegis_oms::domain::order::{Side, TimeInForce};
use aegis_oms::service::order_service::NewOrderRequest;
use rust_decimal_macros::dec;

#[test]
fn deposit_and_withdraw_affects_balance() {
    let mut svc = common::setup_service();

    let balance_before = svc.get_account().balance;
    svc.deposit(dec!(50000)).unwrap();
    assert_eq!(svc.get_account().balance, balance_before + dec!(50000));

    svc.withdraw(dec!(30000)).unwrap();
    assert_eq!(svc.get_account().balance, balance_before + dec!(20000));
}

#[test]
fn leverage_locks_less_margin_for_limit_order() {
    let mut svc = common::setup_service();
    let balance_before = svc.get_account().balance;

    let id = svc
        .submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: aegis_oms::domain::order::OrderType::Limit { price: dec!(65000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        })
        .unwrap();

    let expected = dec!(65000) / dec!(2);
    assert_eq!(svc.get_account().available_balance(), balance_before - expected);

    svc.fill_order(&id, dec!(1.0), dec!(65000)).unwrap();

    assert_eq!(svc.get_account().balance, balance_before);
    assert_eq!(svc.get_account().locked_margin, rust_decimal::Decimal::ZERO);
}

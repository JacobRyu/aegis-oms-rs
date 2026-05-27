#[test]
fn test_negative_unlock_margin() {
    use aegis_oms::domain::account::Account;
    use rust_decimal_macros::dec;

    let mut acc = Account::new("test", "Test", dec!(100000));
    acc.lock_margin(dec!(10000)).unwrap();

    // Try to unlock a negative amount - should this work?
    let result = acc.unlock_margin(dec!(-5000));
    println!("Unlock negative result: {:?}", result);
    println!("Locked margin after: {}", acc.locked_margin);
}

#[test]
fn test_zero_leverage() {
    use aegis_oms::domain::account::MarginCalculator;
    use rust_decimal_macros::dec;

    // This should panic with division by zero
    let margin = MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(0));
    println!("Margin with zero leverage: {}", margin);
}

#[test]
fn test_position_reversal() {
    use aegis_oms::domain::order::Side;
    use aegis_oms::domain::position::Position;
    use rust_decimal_macros::dec;

    let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));

    // Try to reduce by more than position size
    let pnl = pos.reduce(dec!(2.0), dec!(67000));

    println!("PnL: {}", pnl);
    println!("Remaining position: {}", pos.quantity);
    println!("Position is flat: {}", pos.is_flat());
}

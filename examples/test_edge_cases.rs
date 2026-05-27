use aegis_oms::domain::account::{Account, MarginCalculator};
use aegis_oms::domain::order::Side;
use aegis_oms::domain::position::Position;
use rust_decimal_macros::dec;

fn main() {
    println!("=== Testing Edge Cases ===\n");

    // Test 1: Negative unlock amount
    println!("Test 1: Negative unlock amount");
    let mut acc = Account::new("test", "Test", dec!(100000));
    acc.lock_margin(dec!(10000)).unwrap();
    println!("Locked margin: {}", acc.locked_margin);

    match acc.unlock_margin(dec!(-5000)) {
        Ok(_) => {
            println!("WARNING: Unlock negative succeeded!");
            println!("Locked margin after negative unlock: {}", acc.locked_margin);
        }
        Err(e) => println!("Correctly rejected: {}", e),
    }

    println!("\nTest 2: Zero leverage margin calculation");
    match MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(0)) {
        Ok(m) => println!("Margin with zero leverage: {}", m),
        Err(e) => println!("Correctly rejected: {}", e),
    }

    println!("\nTest 3: Position over-reduction");
    let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
    println!("Initial position: {}", pos.quantity);
    let pnl = pos.reduce(dec!(2.0), dec!(67000));
    println!("PnL from reducing 2.0 BTC: {}", pnl);
    println!("Remaining quantity: {}", pos.quantity);
}

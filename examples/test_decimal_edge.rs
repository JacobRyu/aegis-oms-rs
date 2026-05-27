use aegis_oms::domain::account::MarginCalculator;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn main() {
    println!("=== Testing Decimal Edge Cases ===\n");

    // Test with very small leverage (close to zero)
    println!("Test 1: Very small leverage");
    match MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(0.0001)) {
        Ok(m) => println!("Leverage 0.0001: margin = {}", m),
        Err(e) => println!("Rejected: {}", e),
    }

    // Test with very large numbers (overflow protection)
    println!("\nTest 2: Very large price * quantity");
    match MarginCalculator::required_margin(Decimal::MAX / dec!(2), dec!(10), dec!(1)) {
        Ok(m) => println!("Very large calculation: {}", m),
        Err(e) => println!("Correctly rejected overflow: {}", e),
    }

    // Test negative leverage (now rejected)
    println!("\nTest 3: Negative leverage");
    match MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(-2)) {
        Ok(m) => println!("Negative leverage: margin = {}", m),
        Err(e) => println!("Correctly rejected: {}", e),
    }
}

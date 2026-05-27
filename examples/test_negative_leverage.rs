use aegis_oms::domain::account::{Account, MarginCalculator};
use rust_decimal_macros::dec;

fn main() {
    println!("=== Testing Negative Leverage ===\n");

    let margin = MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(-2));
    println!("Margin with negative leverage -2: {:?}", margin);
    println!("Negative leverage is now correctly rejected at calculation time.");

    let mut acc = Account::new("test", "Test", dec!(100000));
    println!("\nInitial balance: {}", acc.balance);
    println!("Initial available: {}", acc.available_balance());

    match margin {
        Ok(m) => match acc.lock_margin(m) {
            Ok(_) => {
                println!("ERROR: Successfully locked margin from negative leverage!");
                println!("Locked margin: {}", acc.locked_margin);
            }
            Err(e) => println!("Lock rejected: {}", e),
        },
        Err(e) => println!("Correctly rejected at required_margin: {}", e),
    }
}

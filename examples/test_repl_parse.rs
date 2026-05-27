use rust_decimal::Decimal;

fn main() {
    println!("=== Testing REPL Parse Behavior ===\n");

    // Simulate what happens in REPL when user enters invalid numbers
    let invalid_inputs = vec!["abc", "12.34.56", "", "NaN", "infinity"];

    for input in invalid_inputs {
        let parsed: Decimal = input.parse().unwrap_or_default();
        println!("Input: {:?} -> Parsed as: {}", input, parsed);
    }

    println!("\nThis means invalid input silently becomes 0, which could pass validations!");
}

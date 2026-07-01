pub mod handlers;
pub mod repl;

use clap::{Parser, Subcommand};
use rust_decimal::Decimal;

#[derive(Parser)]
#[command(name = "aegis-oms", about = "Order Management System for FX and Crypto")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Submit a new order
    Submit {
        #[arg(short, long)]
        instrument: String,
        #[arg(short, long)]
        side: String,
        #[arg(short = 't', long, default_value = "limit")]
        r#type: String,
        #[arg(short, long, default_value = "0")]
        price: Decimal,
        #[arg(short, long)]
        qty: Decimal,
    },
    /// List open orders
    List,
    /// Cancel an order
    Cancel {
        /// Order ID
        id: String,
    },
    /// Show account balance
    Account,
    /// Show open positions
    Positions,
    /// Show trade history
    History {
        /// Filter by instrument (optional)
        #[arg(short, long)]
        instrument: Option<String>,
    },
    /// Show real-time margin status
    Margin {
        /// Mark price for an instrument (format: SYMBOL:PRICE, repeatable)
        #[arg(short = 'p', long = "price")]
        prices: Vec<String>,
    },
    /// Simulate a fill (for testing)
    Fill {
        /// Order ID
        id: String,
        #[arg(short, long)]
        qty: Decimal,
        #[arg(short, long)]
        price: Decimal,
    },
    /// Deposit funds into account
    Deposit {
        /// Amount to deposit
        #[arg(short, long)]
        amount: Decimal,
    },
    /// Withdraw funds from account
    Withdraw {
        /// Amount to withdraw
        #[arg(short, long)]
        amount: Decimal,
    },
    /// Interactive REPL mode
    Repl,
}

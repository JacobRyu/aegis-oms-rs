pub mod handlers;
pub mod repl;

use aegis_oms::domain::account::Account;
use aegis_oms::domain::instrument::{AssetClass, Instrument};
use aegis_oms::infra::event_bus::EventBus;
use aegis_oms::service::order_service::OrderService;
use aegis_oms::service::risk_check::{RiskChecker, RiskLimits};
use clap::{Parser, Subcommand};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub fn default_instruments() -> Vec<Instrument> {
    vec![
        Instrument {
            symbol: "USD/JPY".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        },
        Instrument {
            symbol: "EUR/USD".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.00001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        },
        Instrument {
            symbol: "BTC/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            leverage: dec!(2),
        },
        Instrument {
            symbol: "ETH/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            leverage: dec!(2),
        },
    ]
}

pub fn create_service() -> OrderService {
    let account = Account::new("acc-001", "Default", dec!(100000));
    let instruments = default_instruments();
    let risk = RiskChecker::new(RiskLimits::default());
    let bus = EventBus::new();
    OrderService::new(account, instruments, risk, bus)
}

#[derive(Parser)]
#[command(name = "aegis-oms", about = "Order Management System for FX and Crypto")]
pub struct Cli {
    /// PostgreSQL database URL (e.g. postgres://aegis:aegis_pass@localhost:5432/aegis_oms)
    #[arg(long, env = "DATABASE_URL")]
    pub db: Option<String>,

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

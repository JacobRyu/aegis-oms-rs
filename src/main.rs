mod cli;

use aegis_oms::domain::account::Account;
use aegis_oms::domain::instrument::{AssetClass, Instrument};
use aegis_oms::infra::event_bus::EventBus;
use aegis_oms::service::order_service::OrderService;
use aegis_oms::service::risk_check::{RiskChecker, RiskLimits};
use clap::Parser;
use rust_decimal_macros::dec;

fn default_instruments() -> Vec<Instrument> {
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

fn create_service() -> OrderService {
    let account = Account::new("acc-001", "Default", dec!(100000));
    let instruments = default_instruments();
    let risk = RiskChecker::new(RiskLimits::default());
    let bus = EventBus::new();
    OrderService::new(account, instruments, risk, bus)
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = cli::Cli::parse();
    let mut svc = create_service();

    match cli.command {
        Some(cli::Commands::Submit { instrument, side, r#type, price, qty }) => {
            cli::handlers::handle_submit(&mut svc, &instrument, &side, &r#type, price, qty)
        }
        Some(cli::Commands::List) => cli::handlers::handle_list(&svc),
        Some(cli::Commands::Cancel { id }) => cli::handlers::handle_cancel(&mut svc, &id),
        Some(cli::Commands::Fill { id, qty, price }) => {
            cli::handlers::handle_fill(&mut svc, &id, qty, price)
        }
        Some(cli::Commands::Deposit { amount }) => cli::handlers::handle_deposit(&mut svc, amount),
        Some(cli::Commands::Withdraw { amount }) => {
            cli::handlers::handle_withdraw(&mut svc, amount)
        }
        Some(cli::Commands::Account) => cli::handlers::handle_account(&svc),
        Some(cli::Commands::Positions) => cli::handlers::handle_positions(&svc),
        Some(cli::Commands::History { instrument }) => {
            cli::handlers::handle_history(&svc, instrument.as_deref())
        }
        Some(cli::Commands::Margin { prices }) => {
            let mark_prices = cli::handlers::parse_mark_prices(&prices);
            cli::handlers::handle_margin(&svc, &mark_prices);
        }
        Some(cli::Commands::Repl) | None => cli::repl::run_repl(&mut svc),
    }
}

use aegis_oms::domain::account::Account;
use aegis_oms::domain::instrument::{AssetClass, Instrument};
use aegis_oms::infra::event_bus::EventBus;
use aegis_oms::service::order_service::OrderService;
use aegis_oms::service::risk_check::{RiskChecker, RiskLimits};
use rust_decimal_macros::dec;

pub fn setup_service() -> OrderService {
    let account = Account::new("acc-001", "Test", dec!(100000));
    let instruments = vec![
        Instrument {
            symbol: "BTC/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            leverage: dec!(2),
        },
        Instrument {
            symbol: "USD/JPY".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        },
    ];
    let risk = RiskChecker::new(RiskLimits::default());
    let bus = EventBus::new();
    OrderService::new(account, instruments, risk, bus)
}

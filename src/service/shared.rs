use std::sync::{Arc, Mutex};

use crate::service::order_service::OrderService;

/// Thread-safe wrapper for OrderService.
/// Enables shared ownership across threads for future Web API or multi-threaded REPL usage.
pub type SharedOrderService = Arc<Mutex<OrderService>>;

/// Helper to construct a SharedOrderService from a builder closure.
pub fn create_shared<F>(builder: F) -> SharedOrderService
where
    F: FnOnce() -> OrderService,
{
    Arc::new(Mutex::new(builder()))
}

#[cfg(test)]
mod tests {
    use crate::domain::account::Account;
    use crate::domain::instrument::{AssetClass, Instrument};
    use crate::domain::order::{OrderType, Side, TimeInForce};
    use crate::infra::event_bus::EventBus;
    use crate::service::order_service::{NewOrderRequest, OrderService};
    use crate::service::risk_check::{RiskChecker, RiskLimits};
    use rust_decimal_macros::dec;

    #[test]
    fn shared_service_from_builder() {
        let account = Account::new("acc-001", "Test", dec!(100000));
        let instruments = vec![Instrument {
            symbol: "BTC/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            leverage: dec!(2),
        }];
        let risk = RiskChecker::new(RiskLimits::default());
        let bus = EventBus::new();
        let svc = super::create_shared(|| OrderService::new(account, instruments, risk, bus));
        let mut guard = svc.lock().unwrap();
        let id = guard
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        let order = guard.get_order(&id).unwrap();
        assert_eq!(order.status, crate::domain::order::OrderStatus::Accepted);
    }
}

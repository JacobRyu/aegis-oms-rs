use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::domain::error::Result;
use crate::domain::event::OrderEvent;
use crate::domain::order::Side;
use crate::domain::order::{OrderType, TimeInForce};
use crate::domain::risk_engine::{MarginLevel, MarginStatus};
use crate::infra::alert::{Alert, AlertSender, ConsoleAlertSender};
use crate::service::order_service::{NewOrderRequest, OrderService};

/// 証拠金監視サービス
///
/// `check` を呼ぶたびに `MarginStatus` を計算し、
/// - `MarginCall`: アラート送信
/// - `StopOut`: 全ポジション強制クローズ + アラート送信
pub struct MarginMonitor {
    alert_sender: Box<dyn AlertSender>,
    /// 追証アラート発動水準（デフォルト 100%）
    pub margin_call_ratio: Decimal,
    /// ロスカット発動水準（デフォルト 50%）
    pub stop_out_ratio: Decimal,
    /// 直前の MarginLevel（再通知を避けるため保持）
    last_level: MarginLevel,
}

impl MarginMonitor {
    pub fn new(
        margin_call_ratio: Decimal,
        stop_out_ratio: Decimal,
        alert_sender: Box<dyn AlertSender>,
    ) -> Self {
        Self { alert_sender, margin_call_ratio, stop_out_ratio, last_level: MarginLevel::Normal }
    }

    pub fn with_console_alert(margin_call_ratio: Decimal, stop_out_ratio: Decimal) -> Self {
        Self::new(margin_call_ratio, stop_out_ratio, Box::new(ConsoleAlertSender))
    }

    /// 現在の証拠金ステータスを計算し、必要なアクション（アラート・強制クローズ）を実行する。
    pub fn check(
        &mut self,
        order_svc: &mut OrderService,
        mark_prices: &HashMap<String, Decimal>,
    ) -> Result<MarginStatus> {
        let leverages: HashMap<String, Decimal> = order_svc
            .get_instruments()
            .iter()
            .map(|(sym, inst)| (sym.clone(), inst.leverage))
            .collect();

        let positions: Vec<_> = order_svc.get_positions().into_iter().collect();

        let status = MarginStatus::calculate(
            order_svc.get_account(),
            &positions,
            mark_prices,
            &leverages,
            self.margin_call_ratio,
            self.stop_out_ratio,
        );

        match status.level {
            MarginLevel::StopOut if self.last_level != MarginLevel::StopOut => {
                let alert = Alert::new(MarginLevel::StopOut, status.margin_ratio);
                let _ = self.alert_sender.send(&alert);
                order_svc
                    .event_bus
                    .publish(&OrderEvent::StopOut { margin_ratio: status.margin_ratio });
                self.force_liquidate(order_svc, mark_prices)?;
                self.last_level = MarginLevel::StopOut;
            }
            MarginLevel::MarginCall if self.last_level == MarginLevel::Normal => {
                let alert = Alert::new(MarginLevel::MarginCall, status.margin_ratio);
                let _ = self.alert_sender.send(&alert);
                order_svc
                    .event_bus
                    .publish(&OrderEvent::MarginCall { margin_ratio: status.margin_ratio });
                self.last_level = MarginLevel::MarginCall;
            }
            MarginLevel::Normal => {
                self.last_level = MarginLevel::Normal;
            }
            _ => {}
        }

        Ok(status)
    }

    /// 全ポジションを成行で強制クローズする
    fn force_liquidate(
        &self,
        order_svc: &mut OrderService,
        _mark_prices: &HashMap<String, Decimal>,
    ) -> Result<()> {
        let positions_to_close: Vec<(String, Side, Decimal)> = order_svc
            .get_positions()
            .into_iter()
            .map(|p| (p.instrument.clone(), p.side, p.quantity))
            .collect();

        for (instrument, side, quantity) in positions_to_close {
            let close_side = match side {
                Side::Buy => Side::Sell,
                Side::Sell => Side::Buy,
            };
            let id = order_svc.submit_order(NewOrderRequest {
                instrument: instrument.clone(),
                side: close_side,
                order_type: OrderType::Market,
                quantity,
                time_in_force: TimeInForce::IOC,
            })?;
            // 実際のシステムではマッチングエンジンが約定させるが、
            // シミュレーション環境では呼び出し元が fill_order を呼ぶ
            tracing::warn!(
                order_id = %id,
                instrument = %instrument,
                "Force liquidation order submitted"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::Account;
    use crate::domain::instrument::{AssetClass, Instrument};
    use crate::infra::event_bus::EventBus;
    use crate::service::risk_check::{RiskChecker, RiskLimits};
    use rust_decimal_macros::dec;

    fn setup_svc() -> OrderService {
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
        OrderService::new(account, instruments, risk, bus)
    }

    #[test]
    fn normal_status_no_alert() {
        let mut svc = setup_svc();
        let mut monitor = MarginMonitor::with_console_alert(dec!(100), dec!(50));
        let mark_prices = [("BTC/USD".to_string(), dec!(65000))].into_iter().collect();
        let status = monitor.check(&mut svc, &mark_prices).unwrap();
        assert_eq!(status.level, MarginLevel::Normal);
    }
}

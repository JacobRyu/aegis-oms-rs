use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::domain::composite_order::{
    CompositeOrderStatus, IfdOrder, IfdOrderId, OcoOrder, OcoOrderId,
};
use crate::domain::error::{OmsError, Result};
use crate::domain::event::OrderEvent;
use crate::domain::order::{OrderId, OrderStatus, OrderType};
use crate::service::order_service::OrderService;

/// 複合注文（IFD / OCO）および Stop 系注文のトリガーを管理するサービス
pub struct CompositeOrderService {
    ifd_orders: HashMap<IfdOrderId, IfdOrder>,
    oco_orders: HashMap<OcoOrderId, OcoOrder>,
    /// OCO 検索用: 注文 ID → OCO ID
    order_to_oco: HashMap<OrderId, OcoOrderId>,
    /// IFD 検索用: 親注文 ID → IFD ID
    order_to_ifd: HashMap<OrderId, IfdOrderId>,
}

impl CompositeOrderService {
    pub fn new() -> Self {
        Self {
            ifd_orders: HashMap::new(),
            oco_orders: HashMap::new(),
            order_to_oco: HashMap::new(),
            order_to_ifd: HashMap::new(),
        }
    }

    /// IFD 注文を登録する（primary_id は事前に submit 済みの注文 ID）
    pub fn register_ifd(&mut self, ifd: IfdOrder) {
        self.order_to_ifd.insert(ifd.primary_id, ifd.id.clone());
        self.ifd_orders.insert(ifd.id.clone(), ifd);
    }

    /// OCO 注文を登録する（order_a, order_b は事前に submit 済みの注文 ID）
    pub fn register_oco(&mut self, oco: OcoOrder) {
        self.order_to_oco.insert(oco.order_a_id, oco.id.clone());
        self.order_to_oco.insert(oco.order_b_id, oco.id.clone());
        self.oco_orders.insert(oco.id.clone(), oco);
    }

    /// 価格更新を通知する。Stop / TrailingStop のトリガー判定と TrailingStop の追従を行う。
    /// トリガーされた注文 ID のリストを返す（呼び出し元が `fill_order` 等を実行する）。
    pub fn on_price_update(
        &mut self,
        order_svc: &mut OrderService,
        symbol: &str,
        market_price: Decimal,
    ) -> Result<Vec<OrderId>> {
        let mut triggered_ids = Vec::new();

        let pending_ids = order_svc.find_pending_trigger_orders_for(symbol);

        let mut events: Vec<OrderEvent> = Vec::new();

        for id in pending_ids {
            let order = match order_svc.get_order_mut(&id) {
                Some(o) => o,
                None => continue,
            };

            order.update_trailing_best(market_price);

            let order_id = order.id;
            let is_trailing_stop = matches!(order.order_type, OrderType::TrailingStop { .. });
            let best_price = order.best_price;

            match order.check_trigger(market_price)? {
                Some(exec_type) => {
                    events
                        .push(OrderEvent::StopTriggered { order_id, trigger_price: market_price });
                    triggered_ids.push(order_id);
                    // TrailingStop も Market として記録
                    let _ = exec_type;
                }
                None => {
                    if is_trailing_stop && let Some(bp) = best_price {
                        events.push(OrderEvent::TrailingStopUpdated { order_id, best_price: bp });
                    }
                }
            }
        }

        for event in events {
            order_svc.event_bus.publish(&event);
        }

        Ok(triggered_ids)
    }

    /// 約定通知を受けて IFD / OCO の後続処理を実行する
    pub fn on_order_filled(
        &mut self,
        filled_order_id: OrderId,
        order_svc: &mut OrderService,
    ) -> Result<()> {
        self.handle_ifd(filled_order_id, order_svc)?;
        self.handle_oco(filled_order_id, order_svc)?;
        Ok(())
    }

    fn handle_ifd(&mut self, filled_order_id: OrderId, order_svc: &mut OrderService) -> Result<()> {
        let ifd_id = match self.order_to_ifd.get(&filled_order_id) {
            Some(id) => id.clone(),
            None => return Ok(()),
        };

        let ifd = match self.ifd_orders.get_mut(&ifd_id) {
            Some(o) => o,
            None => return Ok(()),
        };

        if ifd.status != CompositeOrderStatus::Active {
            return Ok(());
        }

        // 親注文の状態確認
        let is_fully_filled = order_svc
            .get_order(&filled_order_id)
            .map(|o| o.status == OrderStatus::Filled)
            .unwrap_or(false);

        if is_fully_filled {
            // secondary_req をクローンして submit
            let secondary_req = ifd.secondary_req.clone();
            order_svc.submit_order(secondary_req)?;
            ifd.status = CompositeOrderStatus::Completed;
        }

        Ok(())
    }

    fn handle_oco(&mut self, filled_order_id: OrderId, order_svc: &mut OrderService) -> Result<()> {
        let oco_id = match self.order_to_oco.get(&filled_order_id) {
            Some(id) => id.clone(),
            None => return Ok(()),
        };

        let oco = match self.oco_orders.get_mut(&oco_id) {
            Some(o) => o,
            None => return Ok(()),
        };

        if oco.status != CompositeOrderStatus::Active {
            return Ok(());
        }

        // 反対の注文をキャンセル
        let other_id =
            if oco.order_a_id == filled_order_id { oco.order_b_id } else { oco.order_a_id };

        let cancel_result = order_svc.cancel_order(&other_id);
        // 相手注文が既にキャンセル済み・約定済みの場合は無視
        match cancel_result {
            Ok(()) | Err(OmsError::InvalidStateTransition { .. }) => {}
            Err(e) => return Err(e),
        }

        oco.status = CompositeOrderStatus::Completed;
        Ok(())
    }

    pub fn get_ifd(&self, id: &IfdOrderId) -> Option<&IfdOrder> {
        self.ifd_orders.get(id)
    }

    pub fn get_oco(&self, id: &OcoOrderId) -> Option<&OcoOrder> {
        self.oco_orders.get(id)
    }
}

impl Default for CompositeOrderService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::Account;
    use crate::domain::composite_order::{IfdOrder, OcoOrder};
    use crate::domain::instrument::{AssetClass, Instrument};
    use crate::domain::order::{OrderType, Side, TimeInForce};
    use crate::infra::event_bus::EventBus;
    use crate::service::order_service::NewOrderRequest;
    use crate::service::risk_check::{RiskChecker, RiskLimits};
    use rust_decimal_macros::dec;

    fn setup_svc() -> OrderService {
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

    #[test]
    fn ifd_secondary_submitted_after_primary_fill() {
        let mut svc = setup_svc();
        let mut comp = CompositeOrderService::new();

        // 親: BTC/USD buy limit
        let primary_id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        // IFD 登録: 親約定後に USD/JPY sell limit を発注
        let ifd = IfdOrder::new(
            primary_id,
            NewOrderRequest {
                instrument: "USD/JPY".into(),
                side: Side::Sell,
                order_type: OrderType::Limit { price: dec!(155) },
                quantity: dec!(10000),
                time_in_force: TimeInForce::GTC,
            },
        );
        comp.register_ifd(ifd);

        // 親注文を完全約定
        svc.fill_order(&primary_id, dec!(1.0), dec!(65000)).unwrap();
        comp.on_order_filled(primary_id, &mut svc).unwrap();

        // 子注文が自動発注されたことを確認
        let open = svc.get_open_orders();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].instrument, "USD/JPY");
    }

    #[test]
    fn oco_cancels_other_on_fill() {
        let mut svc = setup_svc();
        let mut comp = CompositeOrderService::new();

        // 2つの注文を発注
        let id_a = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(64000) },
                quantity: dec!(0.5),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        let id_b = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(63000) },
                quantity: dec!(0.5),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        comp.register_oco(OcoOrder::new(id_a, id_b));

        // id_a が約定
        svc.fill_order(&id_a, dec!(0.5), dec!(64000)).unwrap();
        comp.on_order_filled(id_a, &mut svc).unwrap();

        // id_b がキャンセルされたことを確認
        let order_b = svc.get_order(&id_b).unwrap();
        assert_eq!(order_b.status, OrderStatus::Cancelled);

        // オープン注文がないことを確認
        assert!(svc.get_open_orders().is_empty());
    }

    #[test]
    fn stop_order_triggers_on_price_update() {
        let mut svc = setup_svc();
        let mut comp = CompositeOrderService::new();

        // BTC/USD stop sell @ 64000
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Sell,
                order_type: OrderType::Stop { trigger_price: dec!(64000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        // 価格未達
        let triggered = comp.on_price_update(&mut svc, "BTC/USD", dec!(65000)).unwrap();
        assert!(triggered.is_empty());

        // 価格がトリガー水準に到達
        let triggered = comp.on_price_update(&mut svc, "BTC/USD", dec!(64000)).unwrap();
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0], id);
    }
}

use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::domain::account::Account;
use crate::domain::error::{OmsError, Result};
use crate::domain::event::OrderEvent;
use crate::domain::instrument::Instrument;
use crate::domain::order::*;
use crate::domain::position::Position;
use crate::domain::trade::Trade;
use crate::infra::event_bus::EventBus;
use crate::infra::order_store::{InMemoryOrderStore, OrderStore};
use crate::infra::trade_store::InMemoryTradeStore;
use crate::service::risk_check::RiskChecker;

/// 注文作成リクエスト
#[derive(Debug, Clone)]
pub struct NewOrderRequest {
    pub instrument: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub time_in_force: TimeInForce,
}

/// OrderService: 注文ライフサイクルを統合管理
pub struct OrderService {
    pub store: InMemoryOrderStore,
    pub account: Account,
    pub instruments: HashMap<String, Instrument>,
    pub positions: HashMap<String, Position>,
    /// order_id → ロック済み証拠金額
    margin_locks: HashMap<OrderId, Decimal>,
    risk_checker: RiskChecker,
    pub event_bus: EventBus,
    trade_store: InMemoryTradeStore,
}

impl OrderService {
    pub fn new(
        account: Account,
        instruments: Vec<Instrument>,
        risk_checker: RiskChecker,
        event_bus: EventBus,
    ) -> Self {
        let instruments: HashMap<String, Instrument> =
            instruments.into_iter().map(|i| (i.symbol.clone(), i)).collect();
        Self {
            store: InMemoryOrderStore::new(),
            account,
            instruments,
            positions: HashMap::new(),
            margin_locks: HashMap::new(),
            risk_checker,
            event_bus,
            trade_store: InMemoryTradeStore::new(),
        }
    }

    /// 注文を提出する
    pub fn submit_order(&mut self, req: NewOrderRequest) -> Result<OrderId> {
        let instrument = self
            .instruments
            .get(&req.instrument)
            .ok_or_else(|| OmsError::InstrumentNotFound { symbol: req.instrument.clone() })?
            .clone();

        // リスクチェック (数量・価格)
        let open_count = self.store.find_open_orders().len();
        self.risk_checker.validate_order(&req.order_type, req.quantity, open_count)?;

        // 必要証拠金の計算と証拠金ロック
        let price = match req.order_type {
            OrderType::Limit { price } => price,
            // Market / Stop 系は提出時に価格不明のためスキップ（約定時に検証）
            _ => Decimal::ZERO,
        };

        let mut order =
            Order::new(req.instrument, req.side, req.order_type, req.quantity, req.time_in_force);

        // 証拠金チェック & ロック (Limit 注文のみ)
        if price > Decimal::ZERO {
            let required_margin = self.risk_checker.validate_margin(
                self.account.available_balance(),
                price,
                req.quantity,
                instrument.leverage,
            )?;
            self.account.lock_margin(required_margin)?;
            self.margin_locks.insert(order.id, required_margin);
        }

        // Stop 系注文は PendingTrigger、それ以外は Accepted へ
        if order.is_stop_type() {
            order.pending_trigger()?;
        } else {
            order.accept()?;
        }

        let order_id = order.id;
        let is_stop = order.is_stop_type();
        self.store.save(order)?;

        if !is_stop {
            self.event_bus.publish(&OrderEvent::Accepted { order_id });
        }

        Ok(order_id)
    }

    /// 注文をキャンセルする
    pub fn cancel_order(&mut self, id: &OrderId) -> Result<()> {
        let order = self.store.get_mut(id).ok_or(OmsError::OrderNotFound { order_id: *id })?;

        order.cancel()?;

        // 証拠金ロック解除
        if let Some(margin) = self.margin_locks.remove(id) {
            self.account.unlock_margin(margin)?;
        }

        self.event_bus
            .publish(&OrderEvent::Cancelled { order_id: *id, reason: "User requested".into() });

        Ok(())
    }

    /// 約定を適用する (外部マッチングエンジンからの通知を想定)
    pub fn fill_order(&mut self, id: &OrderId, qty: Decimal, price: Decimal) -> Result<()> {
        let order = self.store.get_mut(id).ok_or(OmsError::OrderNotFound { order_id: *id })?;

        order.apply_fill(qty, price)?;

        let is_fully_filled = order.status == OrderStatus::Filled;
        let instrument = order.instrument.clone();
        let side = order.side;
        let is_market_order = matches!(order.order_type, OrderType::Market)
            || matches!(order.order_type, OrderType::Stop { .. } | OrderType::TrailingStop { .. });

        // イベント発行
        let event = if is_fully_filled {
            OrderEvent::Filled { order_id: *id, filled_qty: qty, price }
        } else {
            OrderEvent::PartiallyFilled { order_id: *id, filled_qty: qty, price }
        };
        self.event_bus.publish(&event);

        // Market注文は提出時に証拠金をスキップするため、約定時に証拠金を検証する
        if is_market_order {
            let leverage = self
                .instruments
                .get(&instrument)
                .ok_or_else(|| OmsError::InstrumentNotFound { symbol: instrument.clone() })?
                .leverage;
            self.risk_checker.validate_margin(
                self.account.available_balance(),
                price,
                qty,
                leverage,
            )?;
        }

        // 証拠金: 完全約定時にロック解除 (Limit注文)
        if is_fully_filled && let Some(margin) = self.margin_locks.remove(id) {
            self.account.unlock_margin(margin)?;
        }

        // ポジション更新して実現損益を取得
        let realized_pnl = self.update_position_with_pnl(&instrument, side, qty, price);

        // 約定履歴を記録
        let trade = Trade::new(*id, instrument.clone(), side, qty, price, realized_pnl);
        self.trade_store.save(trade);

        Ok(())
    }

    fn update_position_with_pnl(
        &mut self,
        instrument: &str,
        side: Side,
        qty: Decimal,
        price: Decimal,
    ) -> Option<Decimal> {
        let (reversal_qty, pnl) = if let Some(pos) = self.positions.get_mut(instrument) {
            if pos.side == side {
                pos.add(qty, price);
                return None;
            }
            let original_qty = pos.quantity;
            let pnl = pos.reduce(qty, price);
            self.account.apply_realized_pnl(pnl);
            if pos.is_flat() { (Some(qty - original_qty), Some(pnl)) } else { (None, Some(pnl)) }
        } else {
            self.positions
                .insert(instrument.into(), Position::new(instrument.into(), side, qty, price));
            return None;
        };

        if let Some(remaining) = reversal_qty {
            self.positions.remove(instrument);
            if remaining > Decimal::ZERO {
                self.positions.insert(
                    instrument.into(),
                    Position::new(instrument.into(), side, remaining, price),
                );
            }
        }
        pnl
    }

    pub fn get_order(&self, id: &OrderId) -> Option<&Order> {
        self.store.get(id)
    }

    pub fn get_open_orders(&self) -> Vec<&Order> {
        self.store.find_open_orders()
    }

    pub fn get_positions(&self) -> Vec<&Position> {
        self.positions.values().collect()
    }

    pub fn get_account(&self) -> &Account {
        &self.account
    }

    /// 全ポジションの未実現損益を市場価格で更新する
    ///
    /// `mark_prices` は銘柄シンボル → 現在市場価格のマップ。
    /// 呼び出し元が最新の市場価格を提供することで `Position::unrealized_pnl` が更新される。
    pub fn update_positions_pnl(
        &mut self,
        mark_prices: &std::collections::HashMap<String, Decimal>,
    ) {
        for (symbol, pos) in &mut self.positions {
            if let Some(&mark_price) = mark_prices.get(symbol) {
                pos.update_unrealized_pnl(mark_price);
            }
        }
    }

    /// 約定履歴を取得する
    pub fn get_trade_history(&self, instrument: Option<&str>) -> Vec<&Trade> {
        match instrument {
            Some(sym) => self.trade_store.by_instrument(sym),
            None => self.trade_store.all(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::Account;
    use crate::domain::instrument::{AssetClass, Instrument};
    use crate::service::risk_check::RiskLimits;
    use pretty_assertions::assert_eq;
    use rust_decimal_macros::dec;

    fn setup() -> OrderService {
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
    fn submit_limit_order() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Accepted);
        assert_eq!(svc.get_account().locked_margin, dec!(32500)); // 65000*1/2
    }

    #[test]
    fn submit_insufficient_funds() {
        let mut svc = setup();
        let result = svc.submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: OrderType::Limit { price: dec!(65000) },
            quantity: dec!(10.0), // requires 325,000 margin, only have 100,000
            time_in_force: TimeInForce::GTC,
        });
        assert!(result.is_err());
    }

    #[test]
    fn submit_unknown_instrument() {
        let mut svc = setup();
        let result = svc.submit_order(NewOrderRequest {
            instrument: "DOGE/USD".into(),
            side: Side::Buy,
            order_type: OrderType::Market,
            quantity: dec!(100),
            time_in_force: TimeInForce::GTC,
        });
        assert!(result.is_err());
    }

    #[test]
    fn cancel_order_releases_margin() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        assert_eq!(svc.get_account().locked_margin, dec!(32500));
        svc.cancel_order(&id).unwrap();
        assert_eq!(svc.get_account().locked_margin, Decimal::ZERO);

        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Cancelled);
    }

    #[test]
    fn fill_order_creates_position() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        svc.fill_order(&id, dec!(1.0), dec!(65000)).unwrap();

        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(svc.get_account().locked_margin, Decimal::ZERO);

        let positions = svc.get_positions();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].instrument, "BTC/USD");
        assert_eq!(positions[0].quantity, dec!(1.0));
    }

    #[test]
    fn partial_fill() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(2.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        svc.fill_order(&id, dec!(1.0), dec!(65000)).unwrap();
        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        // margin still locked
        assert!(svc.get_account().locked_margin > Decimal::ZERO);

        svc.fill_order(&id, dec!(1.0), dec!(65000)).unwrap();
        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(svc.get_account().locked_margin, Decimal::ZERO);
    }

    #[test]
    fn position_reversal_excess_creates_opposite_position() {
        let mut svc = setup();

        // Open 1 BTC long
        let id1 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Market,
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        svc.fill_order(&id1, dec!(1.0), dec!(65000)).unwrap();
        assert_eq!(svc.get_positions().len(), 1);

        // Sell 2 BTC (reversal: 1 closes long, 1 opens short)
        let id2 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Sell,
                order_type: OrderType::Market,
                quantity: dec!(2.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        svc.fill_order(&id2, dec!(2.0), dec!(67000)).unwrap();

        let positions = svc.get_positions();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].side, Side::Sell);
        assert_eq!(positions[0].quantity, dec!(1.0));
    }

    #[test]
    fn close_position_applies_pnl() {
        let mut svc = setup();

        // Open long
        let id1 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        svc.fill_order(&id1, dec!(1.0), dec!(65000)).unwrap();

        let balance_before = svc.get_account().balance;

        // Close by selling at higher price
        let id2 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Sell,
                order_type: OrderType::Limit { price: dec!(67000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        svc.fill_order(&id2, dec!(1.0), dec!(67000)).unwrap();

        // Profit = (67000 - 65000) * 1.0 = +2000
        assert_eq!(svc.get_account().balance, balance_before + dec!(2000));
        assert!(svc.get_positions().is_empty());
    }

    #[test]
    fn fx_order_with_leverage() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "USD/JPY".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(150) },
                quantity: dec!(10000),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();

        // margin = 150 * 10000 / 25 = 60000
        assert_eq!(svc.get_account().locked_margin, dec!(60000));
        assert_eq!(svc.get_account().available_balance(), dec!(40000));

        svc.fill_order(&id, dec!(10000), dec!(150)).unwrap();
        assert_eq!(svc.get_account().locked_margin, Decimal::ZERO);
    }

    #[test]
    fn market_order_fill_rejected_when_insufficient_margin() {
        let mut svc = setup();
        // BTC/USD leverage=2x: 5 BTC @ 65000 requires 162,500 margin, only 100,000 available
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Market,
                quantity: dec!(5.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        let result = svc.fill_order(&id, dec!(5.0), dec!(65000));
        assert!(result.is_err(), "Market order fill should fail when margin is insufficient");
    }

    #[test]
    fn update_positions_pnl() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        svc.fill_order(&id, dec!(1.0), dec!(65000)).unwrap();

        let mark_prices = [("BTC/USD".to_string(), dec!(67000))].into_iter().collect();
        svc.update_positions_pnl(&mark_prices);

        let pos = svc.get_positions().into_iter().find(|p| p.instrument == "BTC/USD").unwrap();
        assert_eq!(pos.unrealized_pnl, dec!(2000));
    }
}

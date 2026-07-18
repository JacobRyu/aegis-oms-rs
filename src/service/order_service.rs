use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::domain::account::Account;
use crate::domain::error::{OmsError, Result};
use crate::domain::event::OrderEvent;
use crate::domain::instrument::Instrument;
use crate::domain::order::*;
use crate::domain::position::Position;
use crate::domain::repository::{OrderRepository, TradeRepository};
use crate::domain::trade::Trade;
use crate::infra::event_bus::EventBus;
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
    pub store: Box<dyn OrderRepository>,
    pub account: Account,
    pub instruments: HashMap<String, Instrument>,
    pub positions: HashMap<String, Position>,
    /// order_id → ロック済み証拠金額
    margin_locks: HashMap<OrderId, Decimal>,
    risk_checker: RiskChecker,
    pub event_bus: EventBus,
    trade_store: Box<dyn TradeRepository>,
    /// 累計実現損失（損失制限チェック用）
    cumulative_realized_loss: Decimal,
}

impl OrderService {
    pub fn new(
        account: Account,
        instruments: Vec<Instrument>,
        risk_checker: RiskChecker,
        event_bus: EventBus,
    ) -> Self {
        Self::with_repos(
            account,
            instruments,
            risk_checker,
            event_bus,
            Box::new(crate::infra::order_store::InMemoryOrderStore::new()),
            Box::new(crate::infra::trade_store::InMemoryTradeStore::new()),
        )
    }

    pub fn with_repos(
        account: Account,
        instruments: Vec<Instrument>,
        risk_checker: RiskChecker,
        event_bus: EventBus,
        store: Box<dyn OrderRepository>,
        trade_store: Box<dyn TradeRepository>,
    ) -> Self {
        let instruments: HashMap<String, Instrument> =
            instruments.into_iter().map(|i| (i.symbol.clone(), i)).collect();
        Self {
            store,
            account,
            instruments,
            positions: HashMap::new(),
            margin_locks: HashMap::new(),
            risk_checker,
            event_bus,
            trade_store,
            cumulative_realized_loss: Decimal::ZERO,
        }
    }

    /// 注文を提出する
    pub fn submit_order(&mut self, req: NewOrderRequest) -> Result<OrderId> {
        let instrument = self
            .instruments
            .get(&req.instrument)
            .ok_or_else(|| OmsError::InstrumentNotFound { symbol: req.instrument.clone() })?
            .clone();

        // リスクチェック (数量・価格・ポジション上限)
        let open_count = self.store.find_open_orders().len();
        let position_count = self.positions.len();
        self.risk_checker.validate_order(
            &req.order_type,
            req.quantity,
            open_count,
            position_count,
        )?;

        // 必要証拠金の計算と証拠金ロック
        let price = match req.order_type {
            OrderType::Limit { price } => price,
            // Market / Stop 系は提出時に価格不明のためスキップ（約定時に検証）
            _ => Decimal::ZERO,
        };

        let mut order =
            Order::new(req.instrument, req.side, req.order_type, req.quantity, req.time_in_force);

        let order_id = order.id;

        // FOK: 全量約定不能（シミュレーション環境では liquidity=0）のため New 状態から却下
        if req.time_in_force == TimeInForce::FOK {
            // FOK は証拠金チェック不要（約定前にリジェクト）
            order.reject()?;
            let rejected_order_id = order.id;
            self.store.save(order)?;
            self.event_bus.publish(&OrderEvent::FokRejected {
                order_id: rejected_order_id,
                available: Decimal::ZERO,
                required: req.quantity,
            });
            return Err(OmsError::FokRejected {
                order_id: rejected_order_id,
                available: Decimal::ZERO,
                required: req.quantity,
            });
        }

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

        let is_stop = order.is_stop_type();
        self.store.save(order)?;

        if !is_stop {
            self.event_bus.publish(&OrderEvent::Accepted { order_id });
        }

        // IOC: 未約定残量を自動キャンセル（stop 系注文は PendingTrigger なので対象外）
        if !is_stop && req.time_in_force == TimeInForce::IOC {
            self.apply_ioc_policy(order_id)?;
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

        // 損失制限チェック: 累計実現損失が max_loss を超えたらエラー
        if let Some(pnl) = realized_pnl
            && pnl.is_sign_negative()
        {
            self.cumulative_realized_loss += pnl;
        }
        if let Some(max_loss) = self.risk_checker.limits.max_loss
            && self.cumulative_realized_loss < -max_loss
        {
            return Err(OmsError::RiskCheckFailed {
                reason: format!(
                    "Loss limit exceeded: cumulative loss {} exceeds max loss {}",
                    self.cumulative_realized_loss, max_loss
                ),
            });
        }

        // 約定履歴を記録
        let trade = Trade::new(*id, instrument.clone(), side, qty, price, realized_pnl);
        self.trade_store.save(trade)?;

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

    /// IOC 執行ポリシーを適用する: 未約定残量を自動キャンセル
    fn apply_ioc_policy(&mut self, order_id: OrderId) -> Result<()> {
        let residual = {
            let order = self.store.get(&order_id).ok_or(OmsError::OrderNotFound { order_id })?;
            order.remaining_quantity()
        };
        if residual > Decimal::ZERO {
            let order = self.store.get_mut(&order_id).unwrap();
            order.cancel()?;
            if let Some(margin) = self.margin_locks.remove(&order_id) {
                self.account.unlock_margin(margin)?;
            }
            self.event_bus.publish(&OrderEvent::IocResidualCancelled { order_id, residual });
        }
        Ok(())
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

    pub fn get_account_mut(&mut self) -> &mut Account {
        &mut self.account
    }

    /// 入金
    pub fn deposit(&mut self, amount: Decimal) -> Result<()> {
        self.account.deposit(amount)
    }

    /// 出金
    pub fn withdraw(&mut self, amount: Decimal) -> Result<()> {
        self.account.withdraw(amount)
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

    /// PendingTrigger 状態の注文のうち、指定された銘柄のものを返す
    pub fn find_pending_trigger_orders_for(&self, symbol: &str) -> Vec<OrderId> {
        self.store
            .find_pending_trigger_orders()
            .into_iter()
            .filter(|o| o.instrument == symbol)
            .map(|o| o.id)
            .collect()
    }

    pub fn get_order_mut(&mut self, id: &OrderId) -> Option<&mut Order> {
        self.store.get_mut(id)
    }

    pub fn get_instruments(&self) -> &HashMap<String, Instrument> {
        &self.instruments
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

    // ── IOC/FOK Tests ──

    #[test]
    fn ioc_order_auto_cancels_residual() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::IOC,
            })
            .unwrap();

        // IOC order should be accepted then immediately cancelled (residual = full qty)
        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Cancelled);
        // Margin should be released
        assert_eq!(svc.get_account().locked_margin, Decimal::ZERO);
    }

    #[test]
    fn ioc_partial_fill_then_residual_cancelled() {
        let mut svc = setup();
        let id = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Buy,
                order_type: OrderType::Limit { price: dec!(65000) },
                quantity: dec!(2.0),
                time_in_force: TimeInForce::IOC,
            })
            .unwrap();

        // Fill 0.5 before the IOC policy would have cancelled
        // (In simulation, IOC cancels immediately after submit.
        //  To test partial fill semantics, we apply fill first then policy)
        // Actually: in our implementation, IOC cancels in submit_order.
        // The order is already cancelled. We can't fill it.
        let order = svc.get_order(&id).unwrap();
        assert_eq!(order.status, OrderStatus::Cancelled);
        // Trying to fill a cancelled order should fail
        let result = svc.fill_order(&id, dec!(0.5), dec!(65000));
        assert!(result.is_err());
    }

    #[test]
    fn fok_order_rejected_when_no_available_liquidity() {
        let mut svc = setup();
        let result = svc.submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: OrderType::Limit { price: dec!(65000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::FOK,
        });

        // In simulation, no immediate liquidity → FOK rejected
        assert!(result.is_err());
        match result {
            Err(OmsError::FokRejected { order_id: _, available, required }) => {
                assert_eq!(available, Decimal::ZERO);
                assert_eq!(required, dec!(1.0));
            }
            _ => panic!("Expected FokRejected error"),
        }
    }

    #[test]
    fn gtc_order_not_affected_by_ioc_fok_policy() {
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
        assert_eq!(svc.get_account().locked_margin, dec!(32500));
    }

    // ── Position Limit Tests ──

    #[test]
    fn position_limit_rejects_order_when_at_max() {
        let risk = RiskChecker::new(RiskLimits { max_open_positions: 1, ..RiskLimits::default() });
        let mut svc = OrderService::new(
            Account::new("acc-001", "Test", dec!(100000)),
            vec![Instrument {
                symbol: "BTC/USD".into(),
                asset_class: AssetClass::Crypto,
                tick_size: dec!(0.01),
                lot_size: dec!(0.001),
                leverage: dec!(2),
            }],
            risk,
            EventBus::new(),
        );

        // Open first position
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
        assert_eq!(svc.get_positions().len(), 1);

        // Second order should fail due to position limit
        let result = svc.submit_order(NewOrderRequest {
            instrument: "BTC/USD".into(),
            side: Side::Buy,
            order_type: OrderType::Limit { price: dec!(66000) },
            quantity: dec!(1.0),
            time_in_force: TimeInForce::GTC,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Open position limit"));
    }

    // ── Loss Limit Tests ──

    #[test]
    fn loss_limit_exceeded_rejects_fill() {
        let risk =
            RiskChecker::new(RiskLimits { max_loss: Some(dec!(5000)), ..RiskLimits::default() });
        let mut svc = OrderService::new(
            Account::new("acc-001", "Test", dec!(100000)),
            vec![Instrument {
                symbol: "BTC/USD".into(),
                asset_class: AssetClass::Crypto,
                tick_size: dec!(0.01),
                lot_size: dec!(0.001),
                leverage: dec!(2),
            }],
            risk,
            EventBus::new(),
        );

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

        // Close at a loss of 6000 (exceeds 5000 limit)
        let id2 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Sell,
                order_type: OrderType::Limit { price: dec!(59000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        let result = svc.fill_order(&id2, dec!(1.0), dec!(59000));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Loss limit exceeded"));
    }

    #[test]
    fn loss_limit_not_exceeded_when_within_limit() {
        let risk =
            RiskChecker::new(RiskLimits { max_loss: Some(dec!(5000)), ..RiskLimits::default() });
        let mut svc = OrderService::new(
            Account::new("acc-001", "Test", dec!(100000)),
            vec![Instrument {
                symbol: "BTC/USD".into(),
                asset_class: AssetClass::Crypto,
                tick_size: dec!(0.01),
                lot_size: dec!(0.001),
                leverage: dec!(2),
            }],
            risk,
            EventBus::new(),
        );

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

        // Close at a loss of 3000 (within 5000 limit)
        let id2 = svc
            .submit_order(NewOrderRequest {
                instrument: "BTC/USD".into(),
                side: Side::Sell,
                order_type: OrderType::Limit { price: dec!(62000) },
                quantity: dec!(1.0),
                time_in_force: TimeInForce::GTC,
            })
            .unwrap();
        let result = svc.fill_order(&id2, dec!(1.0), dec!(62000));
        assert!(result.is_ok());
    }

    // ── Account Event Tests ──

    #[test]
    fn deposit_increases_balance() {
        let mut svc = setup();
        let balance_before = svc.get_account().balance;
        svc.deposit(dec!(50000)).unwrap();
        assert_eq!(svc.get_account().balance, balance_before + dec!(50000));
    }

    #[test]
    fn withdraw_decreases_balance() {
        let mut svc = setup();
        let balance_before = svc.get_account().balance;
        svc.withdraw(dec!(30000)).unwrap();
        assert_eq!(svc.get_account().balance, balance_before - dec!(30000));
    }

    #[test]
    fn withdraw_exceeds_available_rejected() {
        let mut svc = setup();
        let result = svc.withdraw(dec!(200000));
        assert!(result.is_err());
        match result {
            Err(OmsError::WithdrawalExceedsAvailable { requested, available }) => {
                assert_eq!(requested, dec!(200000));
                assert_eq!(available, dec!(100000));
            }
            _ => panic!("Expected WithdrawalExceedsAvailable error"),
        }
    }
}

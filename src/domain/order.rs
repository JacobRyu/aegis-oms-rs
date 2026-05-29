use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::error::{OmsError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderId(pub Ulid);

impl OrderId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for OrderId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for OrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Buy => write!(f, "Buy"),
            Side::Sell => write!(f, "Sell"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit {
        price: Decimal,
    },
    /// 逆指値: trigger_price に達したら Market として執行
    Stop {
        trigger_price: Decimal,
    },
    /// 逆指値指値: trigger_price に達したら Limit { limit_price } として発注
    StopLimit {
        trigger_price: Decimal,
        limit_price: Decimal,
    },
    /// トレーリングストップ: best_price から trail_amount 離れたら Market として執行
    TrailingStop {
        trail_amount: Decimal,
    },
}

impl std::fmt::Display for OrderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderType::Market => write!(f, "Market"),
            OrderType::Limit { price } => write!(f, "Limit@{price}"),
            OrderType::Stop { trigger_price } => write!(f, "Stop@{trigger_price}"),
            OrderType::StopLimit { trigger_price, limit_price } => {
                write!(f, "StopLimit@{trigger_price}/{limit_price}")
            }
            OrderType::TrailingStop { trail_amount } => write!(f, "TrailingStop({trail_amount})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good Till Cancel
    GTC,
    /// Immediate or Cancel
    IOC,
    /// Fill or Kill
    FOK,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    /// Stop / StopLimit / TrailingStop 注文がトリガー待ち状態
    PendingTrigger,
    Accepted,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

impl OrderStatus {
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            OrderStatus::New
                | OrderStatus::PendingTrigger
                | OrderStatus::Accepted
                | OrderStatus::PartiallyFilled
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected)
    }

    /// 許可されるステート遷移を検証する
    pub fn can_transition_to(&self, next: OrderStatus) -> bool {
        matches!(
            (self, next),
            (OrderStatus::New, OrderStatus::PendingTrigger)
                | (OrderStatus::New, OrderStatus::Accepted)
                | (OrderStatus::New, OrderStatus::Rejected)
                | (OrderStatus::PendingTrigger, OrderStatus::Accepted)
                | (OrderStatus::PendingTrigger, OrderStatus::Cancelled)
                | (OrderStatus::Accepted, OrderStatus::PartiallyFilled)
                | (OrderStatus::Accepted, OrderStatus::Filled)
                | (OrderStatus::Accepted, OrderStatus::Cancelled)
                | (OrderStatus::PartiallyFilled, OrderStatus::PartiallyFilled)
                | (OrderStatus::PartiallyFilled, OrderStatus::Filled)
                | (OrderStatus::PartiallyFilled, OrderStatus::Cancelled)
        )
    }
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OrderStatus::New => "New",
            OrderStatus::PendingTrigger => "PendingTrigger",
            OrderStatus::Accepted => "Accepted",
            OrderStatus::PartiallyFilled => "PartiallyFilled",
            OrderStatus::Filled => "Filled",
            OrderStatus::Cancelled => "Cancelled",
            OrderStatus::Rejected => "Rejected",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub instrument: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub time_in_force: TimeInForce,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// TrailingStop 注文が追従する最良価格（その他注文では None）
    pub best_price: Option<Decimal>,
}

impl Order {
    pub fn new(
        instrument: String,
        side: Side,
        order_type: OrderType,
        quantity: Decimal,
        time_in_force: TimeInForce,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: OrderId::new(),
            instrument,
            side,
            order_type,
            quantity,
            filled_quantity: Decimal::ZERO,
            time_in_force,
            status: OrderStatus::New,
            created_at: now,
            updated_at: now,
            best_price: None,
        }
    }

    pub fn remaining_quantity(&self) -> Decimal {
        self.quantity - self.filled_quantity
    }

    /// ステータスを遷移させる (不正遷移はエラー)
    pub fn transition_to(&mut self, next: OrderStatus) -> Result<()> {
        if !self.status.can_transition_to(next) {
            return Err(OmsError::InvalidStateTransition {
                order_id: self.id,
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// 約定を適用する
    pub fn apply_fill(&mut self, qty: Decimal, _price: Decimal) -> Result<()> {
        let new_filled = self.filled_quantity + qty;
        if new_filled > self.quantity {
            return Err(OmsError::OverFill {
                order_id: self.id,
                requested: qty,
                remaining: self.remaining_quantity(),
            });
        }

        self.filled_quantity = new_filled;

        let next_status = if new_filled == self.quantity {
            OrderStatus::Filled
        } else {
            OrderStatus::PartiallyFilled
        };

        self.transition_to(next_status)
    }

    pub fn cancel(&mut self) -> Result<()> {
        self.transition_to(OrderStatus::Cancelled)
    }

    pub fn accept(&mut self) -> Result<()> {
        self.transition_to(OrderStatus::Accepted)
    }

    pub fn reject(&mut self) -> Result<()> {
        self.transition_to(OrderStatus::Rejected)
    }

    /// Stop / StopLimit / TrailingStop 注文を PendingTrigger 状態へ遷移させる
    pub fn pending_trigger(&mut self) -> Result<()> {
        self.transition_to(OrderStatus::PendingTrigger)
    }

    /// Stop / TrailingStop 注文のトリガー条件を評価し、
    /// トリガーされた場合は Accepted へ遷移して対応する OrderType を返す。
    ///
    /// 返り値:
    /// - `Some(OrderType::Market)` — Market として即時執行すべき
    /// - `Some(OrderType::Limit{..})` — StopLimit の Limit 注文として発注すべき
    /// - `None` — トリガー未達
    pub fn check_trigger(&mut self, market_price: Decimal) -> Result<Option<OrderType>> {
        if self.status != OrderStatus::PendingTrigger {
            return Ok(None);
        }
        let triggered = match self.order_type {
            OrderType::Stop { trigger_price } => match self.side {
                Side::Buy => market_price >= trigger_price,
                Side::Sell => market_price <= trigger_price,
            },
            OrderType::StopLimit { trigger_price, .. } => match self.side {
                Side::Buy => market_price >= trigger_price,
                Side::Sell => market_price <= trigger_price,
            },
            OrderType::TrailingStop { trail_amount } => {
                let best = self.best_price.unwrap_or(market_price);
                match self.side {
                    Side::Buy => market_price >= best + trail_amount,
                    Side::Sell => market_price <= best - trail_amount,
                }
            }
            _ => false,
        };

        if triggered {
            let exec_type = match self.order_type {
                OrderType::Stop { .. } => OrderType::Market,
                OrderType::TrailingStop { .. } => OrderType::Market,
                OrderType::StopLimit { limit_price, .. } => OrderType::Limit { price: limit_price },
                other => other,
            };
            self.transition_to(OrderStatus::Accepted)?;
            Ok(Some(exec_type))
        } else {
            Ok(None)
        }
    }

    /// TrailingStop 注文の best_price を市場価格で追従更新する。
    /// 有利な方向（Buy なら上昇、Sell なら下落）のみ更新。
    pub fn update_trailing_best(&mut self, market_price: Decimal) {
        if !matches!(self.order_type, OrderType::TrailingStop { .. }) {
            return;
        }
        if self.status != OrderStatus::PendingTrigger {
            return;
        }
        self.best_price = Some(match self.side {
            Side::Buy => self.best_price.map_or(market_price, |b| b.min(market_price)),
            Side::Sell => self.best_price.map_or(market_price, |b| b.max(market_price)),
        });
    }

    /// Limit 注文の場合に価格を返す
    pub fn limit_price(&self) -> Option<Decimal> {
        match self.order_type {
            OrderType::Limit { price } => Some(price),
            _ => None,
        }
    }

    /// Stop 系注文かどうか
    pub fn is_stop_type(&self) -> bool {
        matches!(
            self.order_type,
            OrderType::Stop { .. } | OrderType::StopLimit { .. } | OrderType::TrailingStop { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_limit_order() -> Order {
        Order::new(
            "BTC/USD".into(),
            Side::Buy,
            OrderType::Limit { price: dec!(65000) },
            dec!(1.0),
            TimeInForce::GTC,
        )
    }

    #[test]
    fn new_order_has_correct_defaults() {
        let order = sample_limit_order();
        assert_eq!(order.status, OrderStatus::New);
        assert_eq!(order.filled_quantity, Decimal::ZERO);
        assert_eq!(order.remaining_quantity(), dec!(1.0));
    }

    #[test]
    fn accept_from_new() {
        let mut order = sample_limit_order();
        assert!(order.accept().is_ok());
        assert_eq!(order.status, OrderStatus::Accepted);
    }

    #[test]
    fn reject_from_new() {
        let mut order = sample_limit_order();
        assert!(order.reject().is_ok());
        assert_eq!(order.status, OrderStatus::Rejected);
    }

    #[test]
    fn full_fill_lifecycle() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        order.apply_fill(dec!(0.5), dec!(65000)).unwrap();
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_quantity, dec!(0.5));

        order.apply_fill(dec!(0.5), dec!(65100)).unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert_eq!(order.remaining_quantity(), Decimal::ZERO);
    }

    #[test]
    fn cancel_from_accepted() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        assert!(order.cancel().is_ok());
        assert_eq!(order.status, OrderStatus::Cancelled);
    }

    #[test]
    fn cancel_from_partially_filled() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        order.apply_fill(dec!(0.3), dec!(65000)).unwrap();
        assert!(order.cancel().is_ok());
        assert_eq!(order.status, OrderStatus::Cancelled);
    }

    #[test]
    fn cannot_cancel_from_filled() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        order.apply_fill(dec!(1.0), dec!(65000)).unwrap();
        assert!(order.cancel().is_err());
    }

    #[test]
    fn cannot_fill_from_new() {
        let mut order = sample_limit_order();
        assert!(order.apply_fill(dec!(0.5), dec!(65000)).is_err());
    }

    #[test]
    fn overfill_rejected() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        let result = order.apply_fill(dec!(1.5), dec!(65000));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_transition_error() {
        let mut order = sample_limit_order();
        order.accept().unwrap();
        order.apply_fill(dec!(1.0), dec!(65000)).unwrap();
        // Filled → Accepted should fail
        let result = order.transition_to(OrderStatus::Accepted);
        assert!(result.is_err());
    }

    #[test]
    fn status_is_open_and_terminal() {
        assert!(OrderStatus::New.is_open());
        assert!(OrderStatus::PendingTrigger.is_open());
        assert!(OrderStatus::Accepted.is_open());
        assert!(OrderStatus::PartiallyFilled.is_open());
        assert!(!OrderStatus::Filled.is_open());
        assert!(!OrderStatus::Cancelled.is_open());
        assert!(!OrderStatus::Rejected.is_open());

        assert!(OrderStatus::Filled.is_terminal());
        assert!(OrderStatus::Cancelled.is_terminal());
        assert!(OrderStatus::Rejected.is_terminal());
        assert!(!OrderStatus::New.is_terminal());
        assert!(!OrderStatus::PendingTrigger.is_terminal());
    }

    #[test]
    fn stop_buy_triggers_when_price_reaches_trigger() {
        let mut order = Order::new(
            "BTC/USD".into(),
            Side::Buy,
            OrderType::Stop { trigger_price: dec!(66000) },
            dec!(1.0),
            TimeInForce::GTC,
        );
        order.pending_trigger().unwrap();
        // 未達
        assert!(order.check_trigger(dec!(65999)).unwrap().is_none());
        assert_eq!(order.status, OrderStatus::PendingTrigger);
        // 到達
        let exec = order.check_trigger(dec!(66000)).unwrap();
        assert_eq!(exec, Some(OrderType::Market));
        assert_eq!(order.status, OrderStatus::Accepted);
    }

    #[test]
    fn stop_sell_triggers_when_price_drops() {
        let mut order = Order::new(
            "BTC/USD".into(),
            Side::Sell,
            OrderType::Stop { trigger_price: dec!(64000) },
            dec!(1.0),
            TimeInForce::GTC,
        );
        order.pending_trigger().unwrap();
        assert!(order.check_trigger(dec!(64001)).unwrap().is_none());
        let exec = order.check_trigger(dec!(64000)).unwrap();
        assert_eq!(exec, Some(OrderType::Market));
    }

    #[test]
    fn stop_limit_triggers_returns_limit_type() {
        let mut order = Order::new(
            "BTC/USD".into(),
            Side::Buy,
            OrderType::StopLimit { trigger_price: dec!(66000), limit_price: dec!(66100) },
            dec!(1.0),
            TimeInForce::GTC,
        );
        order.pending_trigger().unwrap();
        let exec = order.check_trigger(dec!(66000)).unwrap();
        assert_eq!(exec, Some(OrderType::Limit { price: dec!(66100) }));
    }

    #[test]
    fn trailing_stop_sell_follows_best_price() {
        let mut order = Order::new(
            "BTC/USD".into(),
            Side::Sell,
            OrderType::TrailingStop { trail_amount: dec!(1000) },
            dec!(1.0),
            TimeInForce::GTC,
        );
        order.pending_trigger().unwrap();

        // 初期 best_price を 68000 に設定
        order.update_trailing_best(dec!(68000));
        assert_eq!(order.best_price, Some(dec!(68000)));

        // 価格上昇 → best_price 更新
        order.update_trailing_best(dec!(70000));
        assert_eq!(order.best_price, Some(dec!(70000)));

        // 価格下落だが trail_amount 未満 → 未トリガー
        let r = order.check_trigger(dec!(69500)).unwrap();
        assert!(r.is_none());

        // trail_amount (1000) 以上の下落 → トリガー
        let r = order.check_trigger(dec!(69000)).unwrap();
        assert_eq!(r, Some(OrderType::Market));
    }
}

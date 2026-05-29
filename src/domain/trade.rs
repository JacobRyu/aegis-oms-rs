use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::order::{OrderId, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeId(pub Ulid);

impl TradeId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for TradeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TradeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 約定履歴レコード
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: TradeId,
    pub order_id: OrderId,
    pub instrument: String,
    pub side: Side,
    pub quantity: Decimal,
    pub price: Decimal,
    /// ポジション縮小時の実現損益（新規建てなら None）
    pub realized_pnl: Option<Decimal>,
    pub executed_at: DateTime<Utc>,
}

impl Trade {
    pub fn new(
        order_id: OrderId,
        instrument: String,
        side: Side,
        quantity: Decimal,
        price: Decimal,
        realized_pnl: Option<Decimal>,
    ) -> Self {
        Self {
            id: TradeId::new(),
            order_id,
            instrument,
            side,
            quantity,
            price,
            realized_pnl,
            executed_at: Utc::now(),
        }
    }
}

impl std::fmt::Display for Trade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {} @ {} pnl={}",
            self.executed_at.format("%Y-%m-%d %H:%M:%S"),
            self.instrument,
            self.side,
            self.quantity,
            self.price,
            self.realized_pnl.map_or("-".into(), |p| p.to_string()),
        )
    }
}

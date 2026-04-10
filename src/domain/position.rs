use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::order::Side;

/// ネッティング方式のポジション (同一銘柄で1つに集約)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub instrument: String,
    pub side: Side,
    pub quantity: Decimal,
    pub avg_price: Decimal,
    pub unrealized_pnl: Decimal,
}

impl Position {
    pub fn new(instrument: String, side: Side, quantity: Decimal, price: Decimal) -> Self {
        Self { instrument, side, quantity, avg_price: price, unrealized_pnl: Decimal::ZERO }
    }

    pub fn is_flat(&self) -> bool {
        self.quantity.is_zero()
    }

    /// 同方向の約定を加算 (ポジション増加)
    pub fn add(&mut self, qty: Decimal, price: Decimal) {
        let total_cost = self.avg_price * self.quantity + price * qty;
        self.quantity += qty;
        if !self.quantity.is_zero() {
            self.avg_price = total_cost / self.quantity;
        }
    }

    /// 反対方向の約定で減算 (ポジション減少)。実現損益を返す。
    pub fn reduce(&mut self, qty: Decimal, price: Decimal) -> Decimal {
        let close_qty = qty.min(self.quantity);
        let pnl = match self.side {
            Side::Buy => (price - self.avg_price) * close_qty,
            Side::Sell => (self.avg_price - price) * close_qty,
        };
        self.quantity -= close_qty;
        if self.quantity.is_zero() {
            self.avg_price = Decimal::ZERO;
        }
        pnl
    }

    /// 現在の市場価格で未実現損益を更新
    pub fn update_unrealized_pnl(&mut self, market_price: Decimal) {
        self.unrealized_pnl = match self.side {
            Side::Buy => (market_price - self.avg_price) * self.quantity,
            Side::Sell => (self.avg_price - market_price) * self.quantity,
        };
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {} @ {}", self.instrument, self.side, self.quantity, self.avg_price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn new_position() {
        let pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
        assert_eq!(pos.quantity, dec!(1.0));
        assert_eq!(pos.avg_price, dec!(65000));
        assert!(!pos.is_flat());
    }

    #[test]
    fn add_same_direction() {
        let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
        pos.add(dec!(1.0), dec!(67000));
        assert_eq!(pos.quantity, dec!(2.0));
        assert_eq!(pos.avg_price, dec!(66000)); // (65000 + 67000) / 2
    }

    #[test]
    fn reduce_with_profit() {
        let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(2.0), dec!(65000));
        let pnl = pos.reduce(dec!(1.0), dec!(67000));
        assert_eq!(pnl, dec!(2000)); // (67000 - 65000) * 1.0
        assert_eq!(pos.quantity, dec!(1.0));
        assert_eq!(pos.avg_price, dec!(65000)); // avg_price unchanged
    }

    #[test]
    fn reduce_with_loss() {
        let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
        let pnl = pos.reduce(dec!(1.0), dec!(63000));
        assert_eq!(pnl, dec!(-2000));
        assert!(pos.is_flat());
    }

    #[test]
    fn sell_position_pnl() {
        let mut pos = Position::new("ETH/USD".into(), Side::Sell, dec!(10.0), dec!(3500));
        let pnl = pos.reduce(dec!(5.0), dec!(3400));
        assert_eq!(pnl, dec!(500)); // (3500 - 3400) * 5 — profit on short
    }

    #[test]
    fn unrealized_pnl_buy() {
        let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
        pos.update_unrealized_pnl(dec!(67000));
        assert_eq!(pos.unrealized_pnl, dec!(2000));
    }

    #[test]
    fn unrealized_pnl_sell() {
        let mut pos = Position::new("BTC/USD".into(), Side::Sell, dec!(1.0), dec!(65000));
        pos.update_unrealized_pnl(dec!(63000));
        assert_eq!(pos.unrealized_pnl, dec!(2000)); // profit on short
    }

    #[test]
    fn full_close_resets_avg_price() {
        let mut pos = Position::new("BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000));
        pos.reduce(dec!(1.0), dec!(66000));
        assert!(pos.is_flat());
        assert_eq!(pos.avg_price, Decimal::ZERO);
    }
}

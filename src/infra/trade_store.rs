use crate::domain::error::Result;
use crate::domain::repository::TradeRepository;
use crate::domain::trade::Trade;

pub struct InMemoryTradeStore {
    trades: Vec<Trade>,
}

impl InMemoryTradeStore {
    pub fn new() -> Self {
        Self { trades: Vec::new() }
    }

    pub fn save(&mut self, trade: Trade) {
        self.trades.push(trade);
    }

    /// 全約定履歴を新しい順で返す
    pub fn all(&self) -> Vec<&Trade> {
        self.trades.iter().rev().collect()
    }

    /// 銘柄でフィルタリングした約定履歴を新しい順で返す
    pub fn by_instrument(&self, instrument: &str) -> Vec<&Trade> {
        self.trades.iter().rev().filter(|t| t.instrument == instrument).collect()
    }
}

impl Default for InMemoryTradeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TradeRepository for InMemoryTradeStore {
    fn save(&mut self, trade: Trade) -> Result<()> {
        self.trades.push(trade);
        Ok(())
    }

    fn all(&self) -> Vec<&Trade> {
        self.trades.iter().rev().collect()
    }

    fn by_instrument(&self, symbol: &str) -> Vec<&Trade> {
        self.trades.iter().rev().filter(|t| t.instrument == symbol).collect()
    }

    fn load_all_owned(&self) -> Result<Vec<Trade>> {
        Ok(self.trades.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::{OrderId, Side};
    use rust_decimal_macros::dec;

    #[test]
    fn save_and_retrieve() {
        let mut store = InMemoryTradeStore::new();
        let t1 =
            Trade::new(OrderId::new(), "BTC/USD".into(), Side::Buy, dec!(1.0), dec!(65000), None);
        let t2 = Trade::new(
            OrderId::new(),
            "USD/JPY".into(),
            Side::Sell,
            dec!(10000),
            dec!(150),
            Some(dec!(500)),
        );
        store.save(t1);
        store.save(t2);

        assert_eq!(store.all().len(), 2);
        assert_eq!(store.by_instrument("BTC/USD").len(), 1);
        assert_eq!(store.by_instrument("ETH/USD").len(), 0);
    }
}

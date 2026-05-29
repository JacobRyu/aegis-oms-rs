use std::collections::HashMap;

use crate::domain::error::Result;
use crate::domain::order::{Order, OrderId};

pub trait OrderStore {
    fn save(&mut self, order: Order) -> Result<()>;
    fn get(&self, id: &OrderId) -> Option<&Order>;
    fn get_mut(&mut self, id: &OrderId) -> Option<&mut Order>;
    fn find_by_instrument(&self, symbol: &str) -> Vec<&Order>;
    fn find_open_orders(&self) -> Vec<&Order>;
}

#[derive(Debug, Default)]
pub struct InMemoryOrderStore {
    orders: HashMap<OrderId, Order>,
}

impl InMemoryOrderStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl OrderStore for InMemoryOrderStore {
    fn save(&mut self, order: Order) -> Result<()> {
        self.orders.insert(order.id, order);
        Ok(())
    }

    fn get(&self, id: &OrderId) -> Option<&Order> {
        self.orders.get(id)
    }

    fn get_mut(&mut self, id: &OrderId) -> Option<&mut Order> {
        self.orders.get_mut(id)
    }

    fn find_by_instrument(&self, symbol: &str) -> Vec<&Order> {
        self.orders.values().filter(|o| o.instrument == symbol).collect()
    }

    fn find_open_orders(&self) -> Vec<&Order> {
        self.orders.values().filter(|o| o.status.is_open()).collect()
    }
}

impl InMemoryOrderStore {
    pub fn find_pending_trigger_orders(&self) -> Vec<&Order> {
        self.orders
            .values()
            .filter(|o| o.status == crate::domain::order::OrderStatus::PendingTrigger)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::*;
    use rust_decimal_macros::dec;

    fn make_order(instrument: &str) -> Order {
        Order::new(
            instrument.into(),
            Side::Buy,
            OrderType::Limit { price: dec!(65000) },
            dec!(1.0),
            TimeInForce::GTC,
        )
    }

    #[test]
    fn save_and_get() {
        let mut store = InMemoryOrderStore::new();
        let order = make_order("BTC/USD");
        let id = order.id;
        store.save(order).unwrap();

        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.instrument, "BTC/USD");
    }

    #[test]
    fn find_by_instrument() {
        let mut store = InMemoryOrderStore::new();
        store.save(make_order("BTC/USD")).unwrap();
        store.save(make_order("ETH/USD")).unwrap();
        store.save(make_order("BTC/USD")).unwrap();

        let btc_orders = store.find_by_instrument("BTC/USD");
        assert_eq!(btc_orders.len(), 2);
    }

    #[test]
    fn find_open_orders_excludes_terminal() {
        let mut store = InMemoryOrderStore::new();
        let mut order1 = make_order("BTC/USD");
        order1.accept().unwrap();

        let mut order2 = make_order("ETH/USD");
        order2.accept().unwrap();
        order2.apply_fill(dec!(1.0), dec!(3500)).unwrap(); // Filled

        store.save(order1).unwrap();
        store.save(order2).unwrap();

        let open = store.find_open_orders();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].instrument, "BTC/USD");
    }

    #[test]
    fn get_mut_and_modify() {
        let mut store = InMemoryOrderStore::new();
        let order = make_order("BTC/USD");
        let id = order.id;
        store.save(order).unwrap();

        let order = store.get_mut(&id).unwrap();
        order.accept().unwrap();

        assert_eq!(store.get(&id).unwrap().status, OrderStatus::Accepted);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let store = InMemoryOrderStore::new();
        assert!(store.get(&OrderId::new()).is_none());
    }
}

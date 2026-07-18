use super::error::Result;
use super::order::{Order, OrderId};
use super::trade::Trade;

pub trait OrderRepository: Send {
    fn save(&mut self, order: Order) -> Result<()>;
    fn get(&self, id: &OrderId) -> Option<&Order>;
    fn get_mut(&mut self, id: &OrderId) -> Option<&mut Order>;
    fn find_open_orders(&self) -> Vec<&Order>;
    fn find_pending_trigger_orders(&self) -> Vec<&Order>;
    fn all_orders(&self) -> Vec<&Order>;
}

pub trait TradeRepository: Send {
    fn save(&mut self, trade: Trade) -> Result<()>;
    fn all(&self) -> Vec<&Trade>;
    fn by_instrument(&self, symbol: &str) -> Vec<&Trade>;
}

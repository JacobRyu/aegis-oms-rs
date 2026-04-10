use rust_decimal::Decimal;

use super::order::OrderId;

/// OMS 全体のイベント
#[derive(Debug, Clone)]
pub enum OrderEvent {
    Created { order_id: OrderId },
    Accepted { order_id: OrderId },
    PartiallyFilled { order_id: OrderId, filled_qty: Decimal, price: Decimal },
    Filled { order_id: OrderId, filled_qty: Decimal, price: Decimal },
    Cancelled { order_id: OrderId, reason: String },
    Rejected { order_id: OrderId, reason: String },
}

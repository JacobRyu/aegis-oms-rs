use rust_decimal::Decimal;
use thiserror::Error;

use super::order::{OrderId, OrderStatus};

pub type Result<T> = std::result::Result<T, OmsError>;

#[derive(Debug, Error)]
pub enum OmsError {
    #[error("Invalid state transition for order {order_id}: {from} → {to}")]
    InvalidStateTransition { order_id: OrderId, from: OrderStatus, to: OrderStatus },

    #[error("Over-fill for order {order_id}: requested {requested}, remaining {remaining}")]
    OverFill { order_id: OrderId, requested: Decimal, remaining: Decimal },

    #[error("Order not found: {order_id}")]
    OrderNotFound { order_id: OrderId },

    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: Decimal, available: Decimal },

    #[error("Invalid margin amount: {amount}")]
    InvalidMarginAmount { amount: Decimal },

    #[error("Margin unlock exceeded: unlock {unlock_amount}, locked {locked}")]
    MarginUnlockExceeded { unlock_amount: Decimal, locked: Decimal },

    #[error("Risk check failed: {reason}")]
    RiskCheckFailed { reason: String },

    #[error("Instrument not found: {symbol}")]
    InstrumentNotFound { symbol: String },
}

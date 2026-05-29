use rust_decimal::Decimal;

use super::order::OrderId;

/// OMS 全体のイベント
#[derive(Debug, Clone)]
pub enum OrderEvent {
    Created {
        order_id: OrderId,
    },
    Accepted {
        order_id: OrderId,
    },
    PartiallyFilled {
        order_id: OrderId,
        filled_qty: Decimal,
        price: Decimal,
    },
    Filled {
        order_id: OrderId,
        filled_qty: Decimal,
        price: Decimal,
    },
    Cancelled {
        order_id: OrderId,
        reason: String,
    },
    Rejected {
        order_id: OrderId,
        reason: String,
    },
    /// Stop / TrailingStop 注文がトリガーされた
    StopTriggered {
        order_id: OrderId,
        trigger_price: Decimal,
    },
    /// 証拠金率が追証水準を下回った
    MarginCall {
        margin_ratio: Decimal,
    },
    /// 証拠金率がロスカット水準を下回った（強制決済開始）
    StopOut {
        margin_ratio: Decimal,
    },
    /// TrailingStop の best_price が更新された
    TrailingStopUpdated {
        order_id: OrderId,
        best_price: Decimal,
    },
}

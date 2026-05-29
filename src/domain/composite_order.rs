use crate::domain::order::OrderId;
use crate::service::order_service::NewOrderRequest;

/// IFD（If Done）複合注文
///
/// 親注文（primary）が Filled になった後に子注文（secondary）を自動発注する。
#[derive(Debug, Clone)]
pub struct IfdOrder {
    pub id: IfdOrderId,
    pub primary_id: OrderId,
    pub secondary_req: NewOrderRequest,
    pub status: CompositeOrderStatus,
}

/// OCO（One-Cancels-Other）複合注文
///
/// 2つの注文のどちらかが約定したとき、もう一方を自動キャンセルする。
#[derive(Debug, Clone)]
pub struct OcoOrder {
    pub id: OcoOrderId,
    pub order_a_id: OrderId,
    pub order_b_id: OrderId,
    pub status: CompositeOrderStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeOrderStatus {
    Active,
    Completed,
    Cancelled,
}

pub type IfdOrderId = String;
pub type OcoOrderId = String;

impl IfdOrder {
    pub fn new(primary_id: OrderId, secondary_req: NewOrderRequest) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            primary_id,
            secondary_req,
            status: CompositeOrderStatus::Active,
        }
    }
}

impl OcoOrder {
    pub fn new(order_a_id: OrderId, order_b_id: OrderId) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            order_a_id,
            order_b_id,
            status: CompositeOrderStatus::Active,
        }
    }
}

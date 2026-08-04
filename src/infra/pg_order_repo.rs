use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use tokio::runtime::Runtime;

use crate::domain::error::{OmsError, Result};
use crate::domain::order::*;
use crate::domain::repository::OrderRepository;

#[derive(Debug, FromRow)]
struct OrderRow {
    id: String,
    instrument: String,
    side: String,
    order_type: String,
    price: Option<Decimal>,
    quantity: Decimal,
    filled_quantity: Decimal,
    time_in_force: String,
    status: String,
    trigger_price: Option<Decimal>,
    limit_price: Option<Decimal>,
    trail_amount: Option<Decimal>,
    best_price: Option<Decimal>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn side_to_str(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn str_to_side(s: &str) -> Option<Side> {
    match s {
        "buy" => Some(Side::Buy),
        "sell" => Some(Side::Sell),
        _ => None,
    }
}

fn order_type_to_fields(
    typ: &OrderType,
) -> (&'static str, Option<Decimal>, Option<Decimal>, Option<Decimal>) {
    match typ {
        OrderType::Market => ("market", None, None, None),
        OrderType::Limit { price } => ("limit", Some(*price), None, None),
        OrderType::Stop { trigger_price } => ("stop", None, Some(*trigger_price), None),
        OrderType::StopLimit { trigger_price, limit_price } => {
            ("stop_limit", None, Some(*trigger_price), Some(*limit_price))
        }
        OrderType::TrailingStop { trail_amount } => {
            ("trailing_stop", None, None, Some(*trail_amount))
        }
    }
}

fn row_to_order_type(
    type_str: &str,
    price: Option<Decimal>,
    trigger_price: Option<Decimal>,
    limit_price: Option<Decimal>,
    trail_amount: Option<Decimal>,
) -> Option<OrderType> {
    match type_str {
        "market" => Some(OrderType::Market),
        "limit" => Some(OrderType::Limit { price: price? }),
        "stop" => Some(OrderType::Stop { trigger_price: trigger_price? }),
        "stop_limit" => {
            Some(OrderType::StopLimit { trigger_price: trigger_price?, limit_price: limit_price? })
        }
        "trailing_stop" => Some(OrderType::TrailingStop { trail_amount: trail_amount? }),
        _ => None,
    }
}

fn time_in_force_to_str(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::GTC => "gtc",
        TimeInForce::IOC => "ioc",
        TimeInForce::FOK => "fok",
    }
}

fn str_to_time_in_force(s: &str) -> Option<TimeInForce> {
    match s {
        "gtc" => Some(TimeInForce::GTC),
        "ioc" => Some(TimeInForce::IOC),
        "fok" => Some(TimeInForce::FOK),
        _ => None,
    }
}

fn status_to_str(s: OrderStatus) -> &'static str {
    match s {
        OrderStatus::New => "new",
        OrderStatus::PendingTrigger => "pending_trigger",
        OrderStatus::Accepted => "accepted",
        OrderStatus::PartiallyFilled => "partially_filled",
        OrderStatus::Filled => "filled",
        OrderStatus::Cancelled => "cancelled",
        OrderStatus::Rejected => "rejected",
    }
}

fn str_to_status(s: &str) -> Option<OrderStatus> {
    match s {
        "new" => Some(OrderStatus::New),
        "pending_trigger" => Some(OrderStatus::PendingTrigger),
        "accepted" => Some(OrderStatus::Accepted),
        "partially_filled" => Some(OrderStatus::PartiallyFilled),
        "filled" => Some(OrderStatus::Filled),
        "cancelled" => Some(OrderStatus::Cancelled),
        "rejected" => Some(OrderStatus::Rejected),
        _ => None,
    }
}

fn row_to_order(row: OrderRow) -> Option<Order> {
    let side = str_to_side(&row.side)?;
    let order_type = row_to_order_type(
        &row.order_type,
        row.price,
        row.trigger_price,
        row.limit_price,
        row.trail_amount,
    )?;
    let time_in_force = str_to_time_in_force(&row.time_in_force)?;
    let status = str_to_status(&row.status)?;
    Some(Order {
        id: OrderId(ulid::Ulid::from_string(&row.id).ok()?),
        instrument: row.instrument,
        side,
        order_type,
        quantity: row.quantity,
        filled_quantity: row.filled_quantity,
        time_in_force,
        status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        best_price: row.best_price,
    })
}

pub struct PgOrderRepository {
    pool: PgPool,
    rt: Runtime,
    cache: HashMap<OrderId, Order>,
}

impl PgOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, rt: Runtime::new().expect("tokio runtime"), cache: HashMap::new() }
    }

    pub fn load_order(&self, id: &OrderId) -> Result<Option<Order>> {
        let id_str = id.to_string();
        let row: Option<OrderRow> = self.rt.block_on(async {
            sqlx::query_as("SELECT id, instrument, side, order_type, price, quantity, filled_quantity, time_in_force, status, trigger_price, limit_price, trail_amount, best_price, created_at, updated_at FROM orders WHERE id = $1")
                .bind(&id_str).fetch_optional(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(row.and_then(row_to_order))
    }

    pub fn load_all_orders(&self) -> Result<Vec<Order>> {
        let rows: Vec<OrderRow> = self.rt.block_on(async {
            sqlx::query_as("SELECT id, instrument, side, order_type, price, quantity, filled_quantity, time_in_force, status, trigger_price, limit_price, trail_amount, best_price, created_at, updated_at FROM orders")
                .fetch_all(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(rows.into_iter().filter_map(row_to_order).collect())
    }

    pub fn load_pending_trigger_orders(&self) -> Result<Vec<Order>> {
        let rows: Vec<OrderRow> = self.rt.block_on(async {
            sqlx::query_as("SELECT id, instrument, side, order_type, price, quantity, filled_quantity, time_in_force, status, trigger_price, limit_price, trail_amount, best_price, created_at, updated_at FROM orders WHERE status = 'pending_trigger'")
                .fetch_all(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(rows.into_iter().filter_map(row_to_order).collect())
    }
}

impl OrderRepository for PgOrderRepository {
    fn save(&mut self, order: Order) -> Result<()> {
        let (type_str, price, trigger_price, limit_price) = order_type_to_fields(&order.order_type);
        let side_str = side_to_str(order.side);
        let tif_str = time_in_force_to_str(order.time_in_force);
        let status_str = status_to_str(order.status);

        self.rt.block_on(async {
            sqlx::query(
                r#"INSERT INTO orders (id, account_id, instrument, side, order_type, price, quantity, filled_quantity, time_in_force, status, trigger_price, limit_price, trail_amount, best_price, created_at, updated_at)
                VALUES ($1, 'acc-001', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                ON CONFLICT (id) DO UPDATE SET status = EXCLUDED.status, filled_quantity = EXCLUDED.filled_quantity, best_price = EXCLUDED.best_price, updated_at = EXCLUDED.updated_at"#,
            )
            .bind(order.id.to_string()).bind(&order.instrument).bind(side_str).bind(type_str)
            .bind(price).bind(order.quantity).bind(order.filled_quantity).bind(tif_str).bind(status_str)
            .bind(trigger_price).bind(limit_price).bind(None::<Decimal>).bind(order.best_price)
            .bind(order.created_at).bind(order.updated_at)
            .execute(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;

        self.cache.insert(order.id, order);
        Ok(())
    }

    fn get(&self, id: &OrderId) -> Option<&Order> {
        self.cache.get(id)
    }

    fn get_mut(&mut self, id: &OrderId) -> Option<&mut Order> {
        self.cache.get_mut(id)
    }

    fn find_open_orders(&self) -> Vec<&Order> {
        self.cache.values().filter(|o| o.status.is_open()).collect()
    }

    fn find_pending_trigger_orders(&self) -> Vec<&Order> {
        self.cache.values().filter(|o| o.status == OrderStatus::PendingTrigger).collect()
    }

    fn all_orders(&self) -> Vec<&Order> {
        self.cache.values().collect()
    }

    fn load_all_owned(&self) -> Result<Vec<Order>> {
        self.load_all_orders()
    }
}

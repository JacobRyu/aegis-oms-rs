use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use tokio::runtime::Runtime;

use crate::domain::error::{OmsError, Result};
use crate::domain::order::Side;
use crate::domain::repository::TradeRepository;
use crate::domain::trade::{Trade, TradeId};

#[derive(Debug, FromRow)]
struct TradeRow {
    id: String,
    order_id: String,
    instrument: String,
    side: String,
    quantity: Decimal,
    price: Decimal,
    realized_pnl: Option<Decimal>,
    executed_at: DateTime<Utc>,
}

fn row_to_trade(row: TradeRow) -> Option<Trade> {
    let side = match row.side.as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return None,
    };
    Some(Trade {
        id: TradeId(ulid::Ulid::from_string(&row.id).ok()?),
        order_id: crate::domain::order::OrderId(ulid::Ulid::from_string(&row.order_id).ok()?),
        instrument: row.instrument,
        side,
        quantity: row.quantity,
        price: row.price,
        realized_pnl: row.realized_pnl,
        executed_at: row.executed_at,
    })
}

pub struct PgTradeRepository {
    pool: PgPool,
    rt: Runtime,
}

impl PgTradeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, rt: Runtime::new().expect("tokio runtime") }
    }

    pub fn load_all(&self) -> Result<Vec<Trade>> {
        let rows: Vec<TradeRow> = self.rt.block_on(async {
            sqlx::query_as("SELECT id, order_id, instrument, side, quantity, price, realized_pnl, executed_at FROM trades ORDER BY executed_at DESC")
                .fetch_all(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(rows.into_iter().filter_map(row_to_trade).collect())
    }
}

impl TradeRepository for PgTradeRepository {
    fn save(&mut self, trade: Trade) -> Result<()> {
        let side_str = match trade.side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        };

        self.rt.block_on(async {
            sqlx::query(
                "INSERT INTO trades (id, order_id, instrument, side, quantity, price, realized_pnl, executed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
            )
            .bind(trade.id.to_string()).bind(trade.order_id.to_string()).bind(&trade.instrument)
            .bind(side_str).bind(trade.quantity).bind(trade.price).bind(trade.realized_pnl)
            .bind(trade.executed_at)
            .execute(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;

        Ok(())
    }

    fn all(&self) -> Vec<&Trade> {
        tracing::warn!("PgTradeRepository::all() returns empty — use load_all() for owned data");
        Vec::new()
    }

    fn by_instrument(&self, _symbol: &str) -> Vec<&Trade> {
        tracing::warn!(
            "PgTradeRepository::by_instrument() returns empty — use load_all() + filter"
        );
        Vec::new()
    }

    fn load_all_owned(&self) -> Result<Vec<Trade>> {
        self.load_all()
    }
}

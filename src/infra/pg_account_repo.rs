use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool};
use tokio::runtime::Runtime;

use crate::domain::account::Account;
use crate::domain::error::{OmsError, Result};
use crate::domain::repository::AccountRepository;

#[derive(Debug, FromRow)]
struct AccountRow {
    id: String,
    name: String,
    balance: Decimal,
    locked_margin: Decimal,
    created_at: DateTime<Utc>,
}

fn row_to_account(row: AccountRow) -> Account {
    Account {
        id: row.id,
        name: row.name,
        balance: row.balance,
        locked_margin: row.locked_margin,
        created_at: row.created_at,
    }
}

pub struct PgAccountRepository {
    pool: PgPool,
    rt: Runtime,
}

impl PgAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, rt: Runtime::new().expect("tokio runtime") }
    }
}

impl AccountRepository for PgAccountRepository {
    fn save(&mut self, account: &Account) -> Result<()> {
        self.rt.block_on(async {
            sqlx::query(
                r#"INSERT INTO accounts (id, name, balance, locked_margin, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, now())
                ON CONFLICT (id) DO UPDATE SET balance = EXCLUDED.balance, locked_margin = EXCLUDED.locked_margin, updated_at = now()"#,
            )
            .bind(&account.id).bind(&account.name).bind(account.balance).bind(account.locked_margin).bind(account.created_at)
            .execute(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(())
    }

    fn load(&self, id: &str) -> Result<Option<Account>> {
        let row: Option<AccountRow> = self.rt.block_on(async {
            sqlx::query_as("SELECT id, name, balance, locked_margin, created_at FROM accounts WHERE id = $1")
                .bind(id).fetch_optional(&self.pool).await
        }).map_err(|e| OmsError::RiskCheckFailed { reason: e.to_string() })?;
        Ok(row.map(row_to_account))
    }
}

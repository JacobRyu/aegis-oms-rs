use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::error::{OmsError, Result};
use super::order::OrderId;

pub type AccountId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub name: String,
    /// 現在残高 (入出金 + 実現損益)
    pub balance: Decimal,
    /// 注文によりロック中の証拠金
    pub locked_margin: Decimal,
    pub created_at: DateTime<Utc>,
}

impl Account {
    pub fn new(id: impl Into<String>, name: impl Into<String>, initial_balance: Decimal) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            balance: initial_balance,
            locked_margin: Decimal::ZERO,
            created_at: Utc::now(),
        }
    }

    /// 利用可能残高 = balance - locked_margin
    pub fn available_balance(&self) -> Decimal {
        self.balance - self.locked_margin
    }

    /// 証拠金をロック (注文受付時)
    pub fn lock_margin(&mut self, amount: Decimal) -> Result<()> {
        if amount <= Decimal::ZERO {
            return Err(OmsError::InvalidMarginAmount { amount });
        }
        if self.available_balance() < amount {
            return Err(OmsError::InsufficientFunds {
                required: amount,
                available: self.available_balance(),
            });
        }
        self.locked_margin += amount;
        Ok(())
    }

    /// 証拠金ロック解除 (キャンセル・約定時)
    pub fn unlock_margin(&mut self, amount: Decimal) -> Result<()> {
        if amount <= Decimal::ZERO {
            return Err(OmsError::InvalidMarginAmount { amount });
        }
        if amount > self.locked_margin {
            return Err(OmsError::MarginUnlockExceeded {
                unlock_amount: amount,
                locked: self.locked_margin,
            });
        }
        self.locked_margin -= amount;
        Ok(())
    }

    /// 実現損益を残高に反映
    pub fn apply_realized_pnl(&mut self, pnl: Decimal) {
        self.balance += pnl;
    }

    /// 入金
    pub fn deposit(&mut self, amount: Decimal) -> Result<()> {
        if amount <= Decimal::ZERO {
            return Err(OmsError::InvalidMarginAmount { amount });
        }
        self.balance += amount;
        Ok(())
    }

    /// 出金（利用可能残高を超える出金は拒否）
    pub fn withdraw(&mut self, amount: Decimal) -> Result<()> {
        if amount <= Decimal::ZERO {
            return Err(OmsError::InvalidMarginAmount { amount });
        }
        if amount > self.available_balance() {
            return Err(OmsError::WithdrawalExceedsAvailable {
                requested: amount,
                available: self.available_balance(),
            });
        }
        self.balance -= amount;
        Ok(())
    }
}

impl std::fmt::Display for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (balance: {}, available: {})",
            self.name,
            self.balance,
            self.available_balance()
        )
    }
}

/// 証拠金計算
#[derive(Debug, Clone)]
pub struct MarginCalculator;

impl MarginCalculator {
    /// 必要証拠金 = (price × quantity) / leverage
    pub fn required_margin(
        price: Decimal,
        quantity: Decimal,
        leverage: Decimal,
    ) -> Result<Decimal> {
        if leverage <= Decimal::ZERO {
            return Err(OmsError::RiskCheckFailed { reason: "Leverage must be positive".into() });
        }
        let notional = price.checked_mul(quantity).ok_or_else(|| OmsError::RiskCheckFailed {
            reason: "Margin calculation overflow".into(),
        })?;
        Ok(notional / leverage)
    }
}

/// 口座関連イベント
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountEvent {
    MarginLocked { account_id: AccountId, order_id: OrderId, amount: Decimal },
    MarginReleased { account_id: AccountId, order_id: OrderId, amount: Decimal },
    PnlApplied { account_id: AccountId, pnl: Decimal, new_balance: Decimal },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_account() -> Account {
        Account::new("acc-001", "Test Account", dec!(100000))
    }

    #[test]
    fn new_account_defaults() {
        let acc = test_account();
        assert_eq!(acc.balance, dec!(100000));
        assert_eq!(acc.locked_margin, Decimal::ZERO);
        assert_eq!(acc.available_balance(), dec!(100000));
    }

    #[test]
    fn lock_margin_success() {
        let mut acc = test_account();
        acc.lock_margin(dec!(10000)).unwrap();
        assert_eq!(acc.locked_margin, dec!(10000));
        assert_eq!(acc.available_balance(), dec!(90000));
    }

    #[test]
    fn lock_margin_insufficient_funds() {
        let mut acc = test_account();
        let result = acc.lock_margin(dec!(200000));
        assert!(result.is_err());
    }

    #[test]
    fn lock_margin_zero_rejected() {
        let mut acc = test_account();
        assert!(acc.lock_margin(Decimal::ZERO).is_err());
    }

    #[test]
    fn unlock_margin_zero_rejected() {
        let mut acc = test_account();
        acc.lock_margin(dec!(5000)).unwrap();
        assert!(acc.unlock_margin(Decimal::ZERO).is_err());
    }

    #[test]
    fn unlock_margin_negative_rejected() {
        let mut acc = test_account();
        acc.lock_margin(dec!(5000)).unwrap();
        assert!(acc.unlock_margin(dec!(-1000)).is_err());
        // negative amount must not inflate locked_margin
        assert_eq!(acc.locked_margin, dec!(5000));
    }

    #[test]
    fn unlock_margin_success() {
        let mut acc = test_account();
        acc.lock_margin(dec!(10000)).unwrap();
        acc.unlock_margin(dec!(5000)).unwrap();
        assert_eq!(acc.locked_margin, dec!(5000));
        assert_eq!(acc.available_balance(), dec!(95000));
    }

    #[test]
    fn unlock_margin_exceeds_locked() {
        let mut acc = test_account();
        acc.lock_margin(dec!(5000)).unwrap();
        assert!(acc.unlock_margin(dec!(10000)).is_err());
    }

    #[test]
    fn apply_realized_pnl_profit() {
        let mut acc = test_account();
        acc.apply_realized_pnl(dec!(5000));
        assert_eq!(acc.balance, dec!(105000));
    }

    #[test]
    fn apply_realized_pnl_loss() {
        let mut acc = test_account();
        acc.apply_realized_pnl(dec!(-3000));
        assert_eq!(acc.balance, dec!(97000));
    }

    #[test]
    fn margin_calculator_required_margin() {
        // BTC/USD: price=65000, qty=1.0, leverage=2x → margin=32500
        let margin = MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(2)).unwrap();
        assert_eq!(margin, dec!(32500));
    }

    #[test]
    fn margin_calculator_fx_leverage() {
        // USD/JPY: price=150, qty=10000, leverage=25x → margin=60000
        let margin = MarginCalculator::required_margin(dec!(150), dec!(10000), dec!(25)).unwrap();
        assert_eq!(margin, dec!(60000));
    }

    #[test]
    fn margin_calculator_zero_leverage_rejected() {
        assert!(MarginCalculator::required_margin(dec!(65000), dec!(1), Decimal::ZERO).is_err());
    }

    #[test]
    fn margin_calculator_negative_leverage_rejected() {
        assert!(MarginCalculator::required_margin(dec!(65000), dec!(1), dec!(-1)).is_err());
    }

    #[test]
    fn full_margin_lifecycle() {
        let mut acc = test_account();

        // 注文受付: 証拠金ロック
        acc.lock_margin(dec!(32500)).unwrap();
        assert_eq!(acc.available_balance(), dec!(67500));

        // 約定: 証拠金解除 + 実現損益
        acc.unlock_margin(dec!(32500)).unwrap();
        acc.apply_realized_pnl(dec!(2000));
        assert_eq!(acc.balance, dec!(102000));
        assert_eq!(acc.available_balance(), dec!(102000));
    }
}

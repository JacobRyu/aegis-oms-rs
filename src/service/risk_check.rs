use rust_decimal::Decimal;

use crate::domain::account::MarginCalculator;
use crate::domain::error::{OmsError, Result};
use crate::domain::order::OrderType;

pub struct RiskLimits {
    pub max_order_quantity: Decimal,
    pub max_open_orders: usize,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self { max_order_quantity: Decimal::new(1_000_000, 0), max_open_orders: 100 }
    }
}

pub struct RiskChecker {
    pub limits: RiskLimits,
}

impl RiskChecker {
    pub fn new(limits: RiskLimits) -> Self {
        Self { limits }
    }

    /// 注文のバリデーション
    pub fn validate_order(
        &self,
        order_type: &OrderType,
        quantity: Decimal,
        open_order_count: usize,
    ) -> Result<()> {
        if quantity <= Decimal::ZERO {
            return Err(OmsError::RiskCheckFailed { reason: "Quantity must be positive".into() });
        }

        if quantity > self.limits.max_order_quantity {
            return Err(OmsError::RiskCheckFailed {
                reason: format!(
                    "Quantity {} exceeds max {}",
                    quantity, self.limits.max_order_quantity
                ),
            });
        }

        if let OrderType::Limit { price } = order_type
            && *price <= Decimal::ZERO
        {
            return Err(OmsError::RiskCheckFailed {
                reason: "Limit price must be positive".into(),
            });
        }

        if open_order_count >= self.limits.max_open_orders {
            return Err(OmsError::RiskCheckFailed {
                reason: format!("Open order limit reached ({})", self.limits.max_open_orders),
            });
        }

        Ok(())
    }

    /// 資金チェック: 利用可能残高 ≥ 必要証拠金
    pub fn validate_margin(
        &self,
        available_balance: Decimal,
        price: Decimal,
        quantity: Decimal,
        leverage: Decimal,
    ) -> Result<Decimal> {
        let required = MarginCalculator::required_margin(price, quantity, leverage);
        if available_balance < required {
            return Err(OmsError::InsufficientFunds { required, available: available_balance });
        }
        Ok(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn checker() -> RiskChecker {
        RiskChecker::new(RiskLimits { max_order_quantity: dec!(1000), max_open_orders: 10 })
    }

    #[test]
    fn valid_limit_order() {
        let c = checker();
        let result = c.validate_order(&OrderType::Limit { price: dec!(100) }, dec!(10), 0);
        assert!(result.is_ok());
    }

    #[test]
    fn valid_market_order() {
        let c = checker();
        let result = c.validate_order(&OrderType::Market, dec!(10), 0);
        assert!(result.is_ok());
    }

    #[test]
    fn zero_quantity_rejected() {
        let c = checker();
        let result = c.validate_order(&OrderType::Market, Decimal::ZERO, 0);
        assert!(result.is_err());
    }

    #[test]
    fn negative_quantity_rejected() {
        let c = checker();
        let result = c.validate_order(&OrderType::Market, dec!(-1), 0);
        assert!(result.is_err());
    }

    #[test]
    fn exceeds_max_quantity() {
        let c = checker();
        let result = c.validate_order(&OrderType::Market, dec!(1001), 0);
        assert!(result.is_err());
    }

    #[test]
    fn negative_limit_price_rejected() {
        let c = checker();
        let result = c.validate_order(&OrderType::Limit { price: dec!(-50) }, dec!(10), 0);
        assert!(result.is_err());
    }

    #[test]
    fn open_order_limit_reached() {
        let c = checker();
        let result = c.validate_order(&OrderType::Market, dec!(10), 10);
        assert!(result.is_err());
    }

    #[test]
    fn margin_check_pass() {
        let c = checker();
        // available=100000, price=65000, qty=1, leverage=2 → required=32500
        let result = c.validate_margin(dec!(100000), dec!(65000), dec!(1), dec!(2));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dec!(32500));
    }

    #[test]
    fn margin_check_insufficient() {
        let c = checker();
        // available=10000, price=65000, qty=1, leverage=2 → required=32500
        let result = c.validate_margin(dec!(10000), dec!(65000), dec!(1), dec!(2));
        assert!(result.is_err());
    }
}

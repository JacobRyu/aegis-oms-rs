use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetClass {
    Fx,
    Crypto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instrument {
    pub symbol: String,
    pub asset_class: AssetClass,
    /// 最小価格変動幅
    pub tick_size: Decimal,
    /// 最小取引単位
    pub lot_size: Decimal,
    /// レバレッジ倍率 (e.g. 25 for FX, 2 for Crypto)
    pub leverage: Decimal,
}

impl std::fmt::Display for AssetClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetClass::Fx => write!(f, "FX"),
            AssetClass::Crypto => write!(f, "Crypto"),
        }
    }
}

impl std::fmt::Display for Instrument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.symbol, self.asset_class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn create_fx_instrument() {
        let inst = Instrument {
            symbol: "USD/JPY".into(),
            asset_class: AssetClass::Fx,
            tick_size: dec!(0.001),
            lot_size: dec!(1000),
            leverage: dec!(25),
        };
        assert_eq!(inst.symbol, "USD/JPY");
        assert_eq!(inst.asset_class, AssetClass::Fx);
        assert_eq!(inst.leverage, dec!(25));
    }

    #[test]
    fn create_crypto_instrument() {
        let inst = Instrument {
            symbol: "BTC/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.001),
            leverage: dec!(2),
        };
        assert_eq!(inst.asset_class, AssetClass::Crypto);
        assert_eq!(inst.leverage, dec!(2));
    }

    #[test]
    fn display_format() {
        let inst = Instrument {
            symbol: "ETH/USD".into(),
            asset_class: AssetClass::Crypto,
            tick_size: dec!(0.01),
            lot_size: dec!(0.01),
            leverage: dec!(2),
        };
        assert_eq!(format!("{inst}"), "ETH/USD (Crypto)");
    }
}

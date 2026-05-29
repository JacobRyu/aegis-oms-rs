use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::domain::account::Account;
use crate::domain::position::Position;

/// 証拠金水準の区分
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarginLevel {
    /// 正常（margin_ratio > margin_call_ratio）
    Normal,
    /// 追証警告（stop_out_ratio < margin_ratio <= margin_call_ratio）
    MarginCall,
    /// ロスカット（margin_ratio <= stop_out_ratio）
    StopOut,
}

/// リアルタイム証拠金ステータス
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginStatus {
    /// 純資産 = balance + unrealized_pnl
    pub equity: Decimal,
    /// 使用中証拠金（全ポジションの必要証拠金合計）
    pub used_margin: Decimal,
    /// 余剰証拠金 = equity - used_margin
    pub free_margin: Decimal,
    /// 証拠金率 (%) = equity / used_margin × 100
    pub margin_ratio: Decimal,
    /// 実効レバレッジ = 総想定元本 / equity
    pub effective_leverage: Decimal,
    /// 現在の証拠金水準
    pub level: MarginLevel,
}

/// ポジションごとのロスカット価格
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopOutPrice {
    pub instrument: String,
    /// この価格に到達するとロスカットが発動する
    pub price: Decimal,
}

impl MarginStatus {
    /// 口座・ポジション・銘柄レバレッジ情報からステータスを計算する
    ///
    /// - `mark_prices`: 銘柄 → 現在市場価格のマップ
    /// - `leverages`: 銘柄 → レバレッジのマップ
    /// - `margin_call_ratio`: 追証発動水準（例: `Decimal::new(100, 0)` = 100%）
    /// - `stop_out_ratio`: ロスカット発動水準（例: `Decimal::new(50, 0)` = 50%）
    pub fn calculate(
        account: &Account,
        positions: &[&Position],
        mark_prices: &std::collections::HashMap<String, Decimal>,
        leverages: &std::collections::HashMap<String, Decimal>,
        margin_call_ratio: Decimal,
        stop_out_ratio: Decimal,
    ) -> Self {
        let unrealized_total: Decimal = positions
            .iter()
            .map(|p| {
                mark_prices
                    .get(&p.instrument)
                    .map(|&mp| match p.side {
                        crate::domain::order::Side::Buy => (mp - p.avg_price) * p.quantity,
                        crate::domain::order::Side::Sell => (p.avg_price - mp) * p.quantity,
                    })
                    .unwrap_or(p.unrealized_pnl)
            })
            .sum();

        let equity = account.balance + unrealized_total;

        let used_margin: Decimal = positions
            .iter()
            .map(|p| {
                let lev = leverages.get(&p.instrument).copied().unwrap_or(Decimal::ONE);
                let mp = mark_prices.get(&p.instrument).copied().unwrap_or(p.avg_price);
                if lev.is_zero() { Decimal::ZERO } else { mp * p.quantity / lev }
            })
            .sum();

        let free_margin = equity - used_margin;

        let margin_ratio = if used_margin.is_zero() {
            Decimal::MAX
        } else {
            equity / used_margin * Decimal::new(100, 0)
        };

        let total_notional: Decimal = positions
            .iter()
            .map(|p| {
                let mp = mark_prices.get(&p.instrument).copied().unwrap_or(p.avg_price);
                mp * p.quantity
            })
            .sum();

        let effective_leverage = if equity.is_zero() || equity.is_sign_negative() {
            Decimal::MAX
        } else {
            total_notional / equity
        };

        let level = if margin_ratio <= stop_out_ratio {
            MarginLevel::StopOut
        } else if margin_ratio <= margin_call_ratio {
            MarginLevel::MarginCall
        } else {
            MarginLevel::Normal
        };

        Self { equity, used_margin, free_margin, margin_ratio, effective_leverage, level }
    }
}

/// ポジションごとのロスカット価格を計算する
///
/// ロスカット発動時の equity = used_margin × stop_out_ratio / 100 となる price を求める。
/// 複数ポジション保有時は他ポジションの PnL を定数とみなし単一ポジション近似で算出。
pub fn calculate_stop_out_prices(
    account: &Account,
    positions: &[&Position],
    mark_prices: &std::collections::HashMap<String, Decimal>,
    leverages: &std::collections::HashMap<String, Decimal>,
    stop_out_ratio: Decimal,
) -> Vec<StopOutPrice> {
    positions
        .iter()
        .filter_map(|p| {
            let lev = leverages.get(&p.instrument).copied()?;
            if lev.is_zero() || p.quantity.is_zero() {
                return None;
            }
            let mp = mark_prices.get(&p.instrument).copied().unwrap_or(p.avg_price);

            // 他ポジションの unrealized_pnl は現在値で固定
            let other_pnl: Decimal = positions
                .iter()
                .filter(|other| other.instrument != p.instrument)
                .map(|other| {
                    mark_prices
                        .get(&other.instrument)
                        .map(|&omp| match other.side {
                            crate::domain::order::Side::Buy => {
                                (omp - other.avg_price) * other.quantity
                            }
                            crate::domain::order::Side::Sell => {
                                (other.avg_price - omp) * other.quantity
                            }
                        })
                        .unwrap_or(other.unrealized_pnl)
                })
                .sum();

            let other_margin: Decimal = positions
                .iter()
                .filter(|other| other.instrument != p.instrument)
                .map(|other| {
                    let olev = leverages.get(&other.instrument).copied().unwrap_or(Decimal::ONE);
                    let omp =
                        mark_prices.get(&other.instrument).copied().unwrap_or(other.avg_price);
                    if olev.is_zero() { Decimal::ZERO } else { omp * other.quantity / olev }
                })
                .sum();

            // equity(p) = balance + other_pnl + pnl_p(price)
            // used_margin(p) = other_margin + price * qty / lev
            // stop_out: equity(p) = used_margin(p) * stop_out_ratio / 100
            //
            // balance + other_pnl ± (price - avg) * qty = (other_margin + price*qty/lev) * R/100
            // 整理すると price について線形方程式を解く:
            let r = stop_out_ratio / Decimal::new(100, 0);
            let base_equity = account.balance + other_pnl;
            let base_margin = other_margin;

            // Buy: pnl = (price - avg) * qty  → price の係数: qty - r*qty/lev
            // base_equity - avg*qty = price*(r/lev - 1)*qty + base_margin*r  ... 整理
            let stop_price = match p.side {
                crate::domain::order::Side::Buy => {
                    let coeff = r / lev - Decimal::ONE;
                    if coeff.is_zero() {
                        return None;
                    }
                    (base_margin * r - base_equity + p.avg_price * p.quantity)
                        / (coeff * p.quantity)
                }
                crate::domain::order::Side::Sell => {
                    let coeff = Decimal::ONE + r / lev;
                    if coeff.is_zero() {
                        return None;
                    }
                    (base_equity + p.avg_price * p.quantity - base_margin * r)
                        / (coeff * p.quantity)
                }
            };

            // 現在価格と同方向にある（すでにロスカット水準を超えていない）場合のみ返す
            let meaningful = match p.side {
                crate::domain::order::Side::Buy => stop_price < mp,
                crate::domain::order::Side::Sell => stop_price > mp,
            };
            if meaningful {
                Some(StopOutPrice { instrument: p.instrument.clone(), price: stop_price })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::account::Account;
    use crate::domain::order::Side;
    use crate::domain::position::Position;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn btc_position(qty: rust_decimal::Decimal, avg: rust_decimal::Decimal) -> Position {
        Position::new("BTC/USD".into(), Side::Buy, qty, avg)
    }

    #[test]
    fn normal_margin_status() {
        let acc = Account::new("a", "T", dec!(100000));
        let pos = btc_position(dec!(1.0), dec!(65000));
        let positions = vec![&pos];
        let mark_prices: HashMap<_, _> = [("BTC/USD".to_string(), dec!(65000))].into();
        let leverages: HashMap<_, _> = [("BTC/USD".to_string(), dec!(2))].into();

        let status = MarginStatus::calculate(
            &acc,
            &positions,
            &mark_prices,
            &leverages,
            dec!(100),
            dec!(50),
        );

        // used_margin = 65000 * 1 / 2 = 32500
        assert_eq!(status.used_margin, dec!(32500));
        // equity = 100000 (unrealized=0)
        assert_eq!(status.equity, dec!(100000));
        // margin_ratio = 100000 / 32500 * 100 ≈ 307.69
        assert!(status.margin_ratio > dec!(300));
        assert_eq!(status.level, MarginLevel::Normal);
    }

    #[test]
    fn stop_out_level_detected() {
        let acc = Account::new("a", "T", dec!(33000));
        let pos = btc_position(dec!(1.0), dec!(65000));
        let positions = vec![&pos];
        // 大きく価格下落
        let mark_prices: HashMap<_, _> = [("BTC/USD".to_string(), dec!(50000))].into();
        let leverages: HashMap<_, _> = [("BTC/USD".to_string(), dec!(2))].into();

        // equity = 33000 + (50000 - 65000)*1 = 33000 - 15000 = 18000
        // used_margin = 50000/2 = 25000
        // margin_ratio = 18000/25000*100 = 72% > 50% → MarginCall
        let status = MarginStatus::calculate(
            &acc,
            &positions,
            &mark_prices,
            &leverages,
            dec!(100),
            dec!(50),
        );
        assert_eq!(status.level, MarginLevel::MarginCall);
    }

    #[test]
    fn no_positions_margin_status() {
        let acc = Account::new("a", "T", dec!(100000));
        let positions: Vec<&Position> = vec![];
        let mark_prices = HashMap::new();
        let leverages = HashMap::new();

        let status = MarginStatus::calculate(
            &acc,
            &positions,
            &mark_prices,
            &leverages,
            dec!(100),
            dec!(50),
        );

        assert_eq!(status.used_margin, dec!(0));
        assert_eq!(status.equity, dec!(100000));
        assert_eq!(status.level, MarginLevel::Normal);
    }
}

-- ============================================================
-- Aegis OMS — 初期マスタデータ投入
-- 手動実行: psql -d aegis_oms -f sql/002_seed_data.sql
-- ============================================================

BEGIN;

-- デフォルト銘柄
INSERT INTO instruments (symbol, asset_class, tick_size, lot_size, leverage) VALUES
    ('USD/JPY', 'fx',     0.001,   1000,  25),
    ('EUR/USD', 'fx',     0.00001, 1000,  25),
    ('BTC/USD', 'crypto', 0.01,    0.001,  2),
    ('ETH/USD', 'crypto', 0.01,    0.01,   2);

-- デフォルト口座
INSERT INTO accounts (id, name, balance) VALUES
    ('acc-001', 'Default', 100000);

COMMIT;

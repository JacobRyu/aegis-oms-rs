-- ============================================================
-- Aegis OMS — Migration: 既存 DB を最新 Rust モデルに同期
-- 対象: 001_create_tables.sql で作成したスキーマから更新
-- 手動実行: psql -d aegis_oms -f sql/002_migrate_order_types.sql
-- ============================================================

BEGIN;

-- 1. order_type enum に stop, stop_limit, trailing_stop を追加
ALTER TYPE order_type ADD VALUE IF NOT EXISTS 'stop';
ALTER TYPE order_type ADD VALUE IF NOT EXISTS 'stop_limit';
ALTER TYPE order_type ADD VALUE IF NOT EXISTS 'trailing_stop';

-- 2. order_status enum に pending_trigger を追加
ALTER TYPE order_status ADD VALUE IF NOT EXISTS 'pending_trigger';

-- 3. orders テーブルにカラム追加（NULL 許容）
ALTER TABLE orders ADD COLUMN IF NOT EXISTS trigger_price NUMERIC;
ALTER TABLE orders ADD COLUMN IF NOT EXISTS limit_price   NUMERIC;
ALTER TABLE orders ADD COLUMN IF NOT EXISTS trail_amount  NUMERIC;
ALTER TABLE orders ADD COLUMN IF NOT EXISTS best_price    NUMERIC;

-- 4. trades テーブルを作成
CREATE TABLE IF NOT EXISTS trades (
    id            TEXT        PRIMARY KEY,
    order_id      TEXT        NOT NULL REFERENCES orders(id),
    instrument    TEXT        NOT NULL REFERENCES instruments(symbol),
    side          order_side  NOT NULL,
    quantity      NUMERIC     NOT NULL CHECK (quantity > 0),
    price         NUMERIC     NOT NULL CHECK (price > 0),
    realized_pnl  NUMERIC,
    executed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_trades_order      ON trades (order_id);
CREATE INDEX IF NOT EXISTS idx_trades_instrument ON trades (instrument);

COMMIT;

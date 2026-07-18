-- ============================================================
-- Aegis OMS — PostgreSQL DDL
-- 手動実行: psql -d aegis_oms -f sql/001_create_tables.sql
-- ============================================================

BEGIN;

-- -----------------------------------------------------------
-- ENUM types
-- -----------------------------------------------------------
CREATE TYPE asset_class AS ENUM ('fx', 'crypto');
CREATE TYPE order_side AS ENUM ('buy', 'sell');
CREATE TYPE order_type AS ENUM ('market', 'limit', 'stop', 'stop_limit', 'trailing_stop');
CREATE TYPE time_in_force AS ENUM ('gtc', 'ioc', 'fok');
CREATE TYPE order_status AS ENUM (
    'new', 'pending_trigger', 'accepted', 'partially_filled',
    'filled', 'cancelled', 'rejected'
);

-- -----------------------------------------------------------
-- 銘柄マスタ
-- -----------------------------------------------------------
CREATE TABLE instruments (
    symbol       TEXT        PRIMARY KEY,
    asset_class  asset_class NOT NULL,
    tick_size    NUMERIC     NOT NULL CHECK (tick_size > 0),
    lot_size     NUMERIC     NOT NULL CHECK (lot_size > 0),
    leverage     NUMERIC     NOT NULL CHECK (leverage > 0),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE  instruments              IS '取引銘柄マスタ';
COMMENT ON COLUMN instruments.tick_size    IS '最小価格変動幅';
COMMENT ON COLUMN instruments.lot_size     IS '最小取引単位';
COMMENT ON COLUMN instruments.leverage     IS 'レバレッジ倍率 (例: FX=25, Crypto=2)';

-- -----------------------------------------------------------
-- 口座
-- -----------------------------------------------------------
CREATE TABLE accounts (
    id            TEXT        PRIMARY KEY,
    name          TEXT        NOT NULL,
    balance       NUMERIC     NOT NULL DEFAULT 0 CHECK (balance >= 0),
    locked_margin NUMERIC     NOT NULL DEFAULT 0 CHECK (locked_margin >= 0),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE  accounts                IS '取引口座';
COMMENT ON COLUMN accounts.balance        IS '現在残高 (入出金 + 実現損益)';
COMMENT ON COLUMN accounts.locked_margin  IS '注文によりロック中の証拠金';

-- -----------------------------------------------------------
-- 注文
-- -----------------------------------------------------------
CREATE TABLE orders (
    id              TEXT           PRIMARY KEY,  -- ULID
    account_id      TEXT           NOT NULL REFERENCES accounts(id),
    instrument      TEXT           NOT NULL REFERENCES instruments(symbol),
    side            order_side     NOT NULL,
    order_type      order_type     NOT NULL,
    price           NUMERIC,                     -- NULL for market orders
    quantity        NUMERIC        NOT NULL CHECK (quantity > 0),
    filled_quantity NUMERIC        NOT NULL DEFAULT 0 CHECK (filled_quantity >= 0),
    time_in_force   time_in_force  NOT NULL DEFAULT 'gtc',
    status          order_status   NOT NULL DEFAULT 'new',
    trigger_price   NUMERIC,                     -- Stop/StopLimit のトリガー価格
    limit_price     NUMERIC,                     -- StopLimit の指値価格
    trail_amount    NUMERIC,                     -- TrailingStop のトレール幅
    best_price      NUMERIC,                     -- TrailingStop 追従中の最良価格
    created_at      TIMESTAMPTZ    NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ    NOT NULL DEFAULT now(),
    CONSTRAINT chk_limit_price CHECK (
        (order_type = 'market' AND price IS NULL)
        OR
        (order_type = 'limit'  AND price IS NOT NULL AND price > 0)
    ),
    CONSTRAINT chk_filled_lte_qty CHECK (filled_quantity <= quantity)
);

CREATE INDEX idx_orders_account    ON orders (account_id);
CREATE INDEX idx_orders_instrument ON orders (instrument);
CREATE INDEX idx_orders_status     ON orders (status) WHERE status NOT IN ('filled', 'cancelled', 'rejected');

COMMENT ON TABLE  orders IS '注文';

-- -----------------------------------------------------------
-- 証拠金ロック (注文単位)
-- -----------------------------------------------------------
CREATE TABLE margin_locks (
    order_id  TEXT    PRIMARY KEY REFERENCES orders(id),
    amount    NUMERIC NOT NULL CHECK (amount > 0),
    locked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE margin_locks IS '注文ごとの証拠金ロック';

-- -----------------------------------------------------------
-- 約定履歴
-- -----------------------------------------------------------
CREATE TABLE fills (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    order_id   TEXT        NOT NULL REFERENCES orders(id),
    quantity   NUMERIC     NOT NULL CHECK (quantity > 0),
    price      NUMERIC     NOT NULL CHECK (price > 0),
    filled_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_fills_order ON fills (order_id);

COMMENT ON TABLE fills IS '約定履歴';

-- -----------------------------------------------------------
-- ポジション (ネッティング方式: 銘柄×口座で1レコード)
-- -----------------------------------------------------------
CREATE TABLE positions (
    account_id     TEXT       NOT NULL REFERENCES accounts(id),
    instrument     TEXT       NOT NULL REFERENCES instruments(symbol),
    side           order_side NOT NULL,
    quantity       NUMERIC    NOT NULL CHECK (quantity >= 0),
    avg_price      NUMERIC    NOT NULL CHECK (avg_price >= 0),
    unrealized_pnl NUMERIC    NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, instrument)
);

COMMENT ON TABLE  positions              IS 'ネッティングポジション';
COMMENT ON COLUMN positions.avg_price    IS '平均取得価格';

-- -----------------------------------------------------------
-- 注文イベントログ (監査証跡)
-- -----------------------------------------------------------
CREATE TABLE order_events (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    order_id   TEXT        NOT NULL REFERENCES orders(id),
    event_type TEXT        NOT NULL,  -- created, accepted, partially_filled, filled, cancelled, rejected
    detail     JSONB,                 -- filled_qty, price, reason etc.
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_order_events_order ON order_events (order_id);

COMMENT ON TABLE order_events IS '注文イベント監査ログ';

-- -----------------------------------------------------------
-- updated_at 自動更新トリガー
-- -----------------------------------------------------------
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_accounts_updated_at
    BEFORE UPDATE ON accounts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER trg_orders_updated_at
    BEFORE UPDATE ON orders
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER trg_positions_updated_at
    BEFORE UPDATE ON positions
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- -----------------------------------------------------------
-- 約定履歴 (trade_history / Trade ドメインモデル対応)
-- -----------------------------------------------------------
CREATE TABLE trades (
    id            TEXT        PRIMARY KEY,  -- ULID
    order_id      TEXT        NOT NULL REFERENCES orders(id),
    instrument    TEXT        NOT NULL REFERENCES instruments(symbol),
    side          order_side  NOT NULL,
    quantity      NUMERIC     NOT NULL CHECK (quantity > 0),
    price         NUMERIC     NOT NULL CHECK (price > 0),
    realized_pnl  NUMERIC,                  -- NULL for new position openings
    executed_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_trades_order      ON trades (order_id);
CREATE INDEX idx_trades_instrument ON trades (instrument);

CREATE TRIGGER trg_trades_updated_at
    BEFORE UPDATE ON trades
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

COMMIT;

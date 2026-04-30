-- ============================================================
-- Aegis OMS — 全テーブル削除 (開発用)
-- 手動実行: psql -d aegis_oms -f sql/000_drop_all.sql
-- ============================================================

BEGIN;

DROP TABLE IF EXISTS order_events CASCADE;
DROP TABLE IF EXISTS fills        CASCADE;
DROP TABLE IF EXISTS margin_locks CASCADE;
DROP TABLE IF EXISTS positions    CASCADE;
DROP TABLE IF EXISTS orders       CASCADE;
DROP TABLE IF EXISTS accounts     CASCADE;
DROP TABLE IF EXISTS instruments  CASCADE;

DROP TYPE IF EXISTS order_status;
DROP TYPE IF EXISTS time_in_force;
DROP TYPE IF EXISTS order_type;
DROP TYPE IF EXISTS order_side;
DROP TYPE IF EXISTS asset_class;

DROP FUNCTION IF EXISTS update_updated_at();

COMMIT;

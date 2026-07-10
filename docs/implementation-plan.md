# Aegis OMS — 実装計画書

> 作成日: 2026-07-01
> 対象リポジトリ: `jacobryu/aegis-oms-rs`
> ベースライン: テスト 77件 全パス、Rust 2024 edition

---

## Executive Summary

既存コードは DDD レイヤ構成・証拠金管理・複合注文・TrailingStop など主要トレーディング機能の実装が完了している。テストカバレッジも充実（77件）。一方で下記の課題が残存する。

1. **未実装機能**: IOC/FOK 執行ロジック、入出金 API、ポジション上限・損失制限チェック
2. **DB 永続化の未接続**: SQL DDL は存在するが Rust 側の接続コードがない。DDL 自体も現 Rust 型と乖離
3. **構造上の技術的負債**: `main.rs` 肥大化、`CompositeOrderService` のカプセル化漏れ、`EventBus` の同期実行
4. **運用基盤の欠如**: tracing subscriber 未初期化、設定管理、スレッド安全性

本計画は 4 フェーズで段階的に品質を高め、最終的に「DB 接続・非同期イベント・設定管理を備えた生産運用可能なリファレンス実装」を目指す。

---

## Requirement Analysis Snapshot

### Goals

- 実装済みコードの技術的負債を解消する
- 未実装機能（IOC/FOK、入出金、リスク制限）を実装する
- SQL DDL と Rust ドメインモデルの乖離を修正し、DB 永続化レイヤを追加する
- CLI/REPL を分割・整理し保守性を向上させる
- イベントバスを非同期対応にする

### Constraints

- Rust 2024 edition / シングルクレート構成を維持
- DDD レイヤ構造（domain ← service ← infra ← main）を崩さない
- 既存 77 テストを破壊しない
- 学習・検証用途であり、本番運用の SLA 要件は不要

### Assumptions

- PostgreSQL 16+ を DB として使用する（sqlx 非同期ドライバ）
- `tracing-subscriber` を標準ロガーとして採用する
- 非同期イベント配信は `tokio` チャネルベースとする（既存同期 API も共存）
- DB 永続化はオプション機能としてビルドフラグで切り替え可能にする

### Missing Information

- 本番デプロイ先環境（Docker / bare metal / cloud）: 現計画では不問
- 対象ユーザーが CLI のみか Web API も必要か: 現フェーズでは CLI に限定

---

## Assumptions

1. 単一リポジトリ・単一開発者を想定した計画とする
2. DB 永続化フェーズでは `sqlx` を使用し、マイグレーションは `sqlx migrate` で管理する
3. 非同期化は `tokio::sync::broadcast` チャネルを用いる
4. 各フェーズ完了の判定は「当該フェーズのテストが全パス」とする

---

## Milestones

| # | フェーズ | 目標 | 完了条件 |
|---|---|---|---|
| P1 | コード品質・技術的負債解消 | 構造改善・tracing 導入・SQL 修正 | `cargo test` 全パス、`cargo clippy` 警告ゼロ |
| P2 | 未実装コア機能の実装 | IOC/FOK / 入出金 / リスク制限追加 | 各機能のユニットテスト追加・パス |
| P3 | DB 永続化レイヤの実装 | sqlx + PostgreSQL 接続 | 統合テスト（Docker Compose）でパス |
| P4 | 運用基盤・非同期化 | 非同期 EventBus・設定管理 | E2E テストパス、CLI 動作確認 |

---

## Detailed Tasks

### Phase 1 — コード品質・技術的負債解消

**P1-1: `main.rs` の分割**

現状の `main.rs`（411行）は CLI パーサ、REPL、各コマンドハンドラがすべて混在している。
以下の構成に分割する。

```
src/
├── cli/
│   ├── mod.rs          # Commands enum, Cli struct (clap)
│   ├── handlers.rs     # handle_submit / handle_fill / ...
│   └── repl.rs         # run_repl()
└── main.rs             # 初期化 + dispatch のみ (~30行)
```

- [ ] `src/cli/` ディレクトリを作成
- [ ] `Commands` enum と `Cli` struct を `cli/mod.rs` へ移動
- [ ] 各 `handle_*` 関数を `cli/handlers.rs` へ移動
- [ ] `run_repl()` を `cli/repl.rs` へ移動
- [ ] `main.rs` は初期化・dispatch のみに縮小（目標 50行以内）
- [ ] `cargo test` 全パス確認

**P1-2: `tracing-subscriber` の初期化**

現状 `tracing::warn!` を使用しているが subscriber が未設定のためログが出力されない。

- [ ] `Cargo.toml` に `tracing-subscriber = { version = "0.3", features = ["env-filter"] }` を追加
- [ ] `main()` 冒頭に `tracing_subscriber::fmt::init()` を追加
- [ ] `RUST_LOG=warn cargo run` でログ出力を確認

**P1-3: SQL DDL の Rust 型との同期**

現在の `sql/001_create_tables.sql` は `order_type`、`order_status` が Rust の現行型と乖離している。

現状の乖離：

| 項目 | SQL (現在) | Rust (現在) |
|---|---|---|
| `order_type` enum | `market, limit` | `market, limit, stop, stop_limit, trailing_stop` |
| `order_status` enum | `new, accepted, ...` | `new, pending_trigger, accepted, ...` |
| `orders.trigger_price` | なし | `OrderType::Stop { trigger_price }` に対応フィールドが必要 |
| `orders.limit_price` | なし | `OrderType::StopLimit { limit_price }` に対応フィールドが必要 |
| `orders.trail_amount` | なし | `OrderType::TrailingStop { trail_amount }` に対応 |
| `orders.best_price` | なし | `Order.best_price: Option<Decimal>` |
| `trade_history` テーブル | なし | `InMemoryTradeStore` に対応 |

- [ ] `sql/002_migrate_order_types.sql`（または `001` を修正）を作成:
  - `order_type` に `stop, stop_limit, trailing_stop` を追加
  - `order_status` に `pending_trigger` を追加
  - `orders` テーブルに `trigger_price, limit_price, trail_amount, best_price` カラムを追加（NULL 許容）
  - `trades` テーブルを新規作成（`id, order_id, instrument, side, quantity, price, realized_pnl, executed_at`）
  - 既存の `chk_limit_price` 制約を拡張
- [ ] `sql/003_seed_data.sql` の銘柄データが最新であることを確認

**P1-4: `CompositeOrderService` のカプセル化改善**

`CompositeOrderService::on_price_update` が `order_svc.store` に直接アクセスしており、DDD のレイヤ境界を侵犯している。

```rust
// 現在（問題）
let pending_ids: Vec<OrderId> = order_svc
    .store
    .find_pending_trigger_orders()  // store への直接アクセス
    ...

// 改善案: OrderService にメソッドを追加
impl OrderService {
    pub fn find_pending_trigger_orders_for(&self, symbol: &str) -> Vec<OrderId> { ... }
    pub fn get_instruments(&self) -> &HashMap<String, Instrument> { ... }
}
```

- [ ] `OrderService` に `find_pending_trigger_orders_for(symbol: &str)` を追加
- [ ] `CompositeOrderService` から `order_svc.store` への直接アクセスを除去
- [ ] `MarginMonitor` から `order_svc.instruments` への直接アクセスを `order_svc.get_leverage(symbol)` 経由に変更
- [ ] 既存テスト全パス確認

---

### Phase 2 — 未実装コア機能の実装

**P2-1: IOC / FOK 執行ポリシーの実装**

`TimeInForce::IOC`（即時約定、残量キャンセル）と `FOK`（全量約定 or 全キャンセル）の実装。

設計:
- `fill_order` の戻り値は変更しない
- `OrderService::submit_order` 完了後、IOC は即時に未約定残量をキャンセル予約、FOK は全量約定できなければ即キャンセル
- シミュレーション環境なので「即時約定チェック」は呼び出し元が明示的に `fill_order` を呼ぶことで対応

```rust
// IOC: submit 後に partial fill を適用 → 残量キャンセル
// FOK: submit 後に「全量約定可能か」チェック → 不可なら即キャンセル
impl OrderService {
    pub fn apply_time_in_force_policy(
        &mut self,
        order_id: OrderId,
        available_qty: Decimal,
    ) -> Result<()> { ... }
}
```

- [ ] `TimeInForce::IOC` ロジックの実装（残量を自動キャンセル）
- [ ] `TimeInForce::FOK` ロジックの実装（全量約定できなければキャンセル）
- [ ] `OrderEvent` に `IocResidualCancelled` / `FokRejected` バリアントを追加
- [ ] ユニットテストを追加（IOC partial fill → cancel, FOK insufficient qty → reject）

**P2-2: 入出金 API の実装**

```rust
impl Account {
    /// 入金
    pub fn deposit(&mut self, amount: Decimal) -> Result<()> { ... }
    /// 出金（利用可能残高を超える出金は拒否）
    pub fn withdraw(&mut self, amount: Decimal) -> Result<()> { ... }
}
```

- [ ] `Account::deposit(amount: Decimal) -> Result<()>` を実装（正値チェック）
- [ ] `Account::withdraw(amount: Decimal) -> Result<()>` を実装（available_balance チェック）
- [ ] `OmsError` に `WithdrawalExceedsAvailable` バリアントを追加
- [ ] `AccountEvent::Deposited` / `AccountEvent::Withdrawn` を追加
- [ ] CLI/REPL に `deposit` / `withdraw` コマンドを追加
- [ ] ユニットテストを追加（正常系・残高不足・ゼロ額・負額）

**P2-3: ポジション上限チェックの実装**

```rust
pub struct RiskLimits {
    // 既存フィールド ...
    /// 最大オープンポジション数（デフォルト 20）
    pub max_open_positions: usize,
    /// 最大損失額（デフォルト None = 無制限）
    pub max_loss: Option<Decimal>,
}
```

- [ ] `RiskLimits` に `max_open_positions: usize`（デフォルト 20）を追加
- [ ] `RiskChecker::validate_order` でオープンポジション数チェックを追加
- [ ] `RiskLimits` に `max_loss: Option<Decimal>` を追加
- [ ] `OrderService::fill_order` 内で実現損失累計が `max_loss` を超えた場合にエラー or アラートを返す
- [ ] ユニットテストを追加

**P2-4: `MarginMonitor` の重複通知抑制改善**

現在 `MarginCall → StopOut` の遷移で MarginCall イベントが重複する可能性がある。ステートマシンで状態遷移を明示的に管理する。

```
Normal → MarginCall → StopOut
  ↑_________________________|  (回復時に Normal へ)
```

- [ ] `MarginMonitor::last_level` の遷移ロジックを見直し
- [ ] `StopOut → Normal` への回復（ポジション解消後）を `check` で正しく処理
- [ ] 遷移パターンをカバーするテストを追加

---

### Phase 3 — DB 永続化レイヤの実装

> **前提**: Phase 1 の SQL DDL 修正が完了していること

**P3-1: sqlx の導入とマイグレーション管理**

- [ ] `Cargo.toml` に `sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "decimal", "chrono", "uuid", "migrate"] }` を追加
- [ ] `DATABASE_URL` 環境変数のサポート（`.env` ファイル）
- [ ] `sqlx::migrate!()` でマイグレーション自動適用
- [ ] `docker-compose.yml` を追加（PostgreSQL 16 コンテナ）

**P3-2: Repository トレイトの定義**

DDD の Repository パターンを適用し、in-memory と DB 実装を差し替え可能にする。

```rust
// domain/repository.rs
pub trait OrderRepository: Send + Sync {
    fn save(&mut self, order: &Order) -> Result<()>;
    fn get(&self, id: &OrderId) -> Option<Order>;
    fn find_open_orders(&self) -> Vec<Order>;
    fn find_pending_trigger_orders(&self) -> Vec<Order>;
}

pub trait TradeRepository: Send + Sync {
    fn save(&mut self, trade: &Trade) -> Result<()>;
    fn all(&self) -> Vec<Trade>;
    fn by_instrument(&self, symbol: &str) -> Vec<Trade>;
}
```

- [ ] `domain/repository.rs` にトレイトを定義
- [ ] `infra/order_store.rs` が `OrderRepository` を実装するようにリファクタ
- [ ] `infra/trade_store.rs` が `TradeRepository` を実装するようにリファクタ
- [ ] `OrderService` がトレイトオブジェクトを受け取るよう変更

**P3-3: PostgreSQL 実装の追加**

```
src/infra/
├── order_store.rs        # InMemoryOrderRepository (既存)
├── trade_store.rs        # InMemoryTradeRepository (既存)
├── pg_order_store.rs     # PgOrderRepository (新規)
└── pg_trade_store.rs     # PgTradeRepository (新規)
```

- [ ] `PgOrderRepository` を実装（`sqlx::PgPool` 経由）
- [ ] `PgTradeRepository` を実装
- [ ] `PgAccountRepository` を実装（`Account` の永続化）
- [ ] `sqlx::test` 属性を使用した統合テストを追加
- [ ] `main.rs` で `--db` フラグ（または `DATABASE_URL` 環境変数）が設定された場合に PG 実装を使用

**P3-4: セッション間状態の復元**

- [ ] CLI 起動時に DB から注文・ポジション・口座を復元する `OrderService::restore_from_db` を実装
- [ ] 注文・ポジション・口座の整合性チェック（DB から復元した状態が破綻していないか検証）

---

### Phase 4 — 運用基盤・非同期化

**P4-1: 非同期 EventBus の実装**

```rust
// infra/async_event_bus.rs
pub struct AsyncEventBus {
    sender: tokio::sync::broadcast::Sender<OrderEvent>,
}

impl AsyncEventBus {
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<OrderEvent> { ... }
    pub async fn publish(&self, event: OrderEvent) { ... }
}
```

- [ ] `tokio::sync::broadcast` ベースの `AsyncEventBus` を実装
- [ ] 既存同期 `EventBus` は後方互換のため残存させる
- [ ] `OrderService` を非同期版(`AsyncOrderService`) と同期版に分離するか、`EventBus` トレイトで抽象化するかを検討
- [ ] イベントログ（発行されたイベントの永続化）の仕組みを追加

**P4-2: 設定管理の実装**

現在、RiskLimits・レバレッジ・口座初期残高がハードコードされている。

```toml
# config/default.toml
[account]
initial_balance = 100000

[risk]
max_order_quantity = 1000000
max_open_orders = 100
max_open_positions = 20
stop_out_ratio = 50
margin_call_ratio = 100

[smtp]
host = ""
port = 587
```

- [ ] `config` クレートを追加（または環境変数 + `config.toml` を手動パース）
- [ ] `AppConfig` 構造体を定義し、`main.rs` 初期化時に読み込む
- [ ] 銘柄マスタを設定ファイルから読み込む（ハードコード脱却）
- [ ] `.env.example` を追加（`DATABASE_URL`, `SMTP_*`, `ALERT_*` の例）

**P4-3: スレッド安全性の確保**

現在 `OrderService` は `Send + Sync` 非対応。将来の Web API 化を見据えて対応する。

```rust
// サービス初期化
use std::sync::{Arc, Mutex};
pub type SharedOrderService = Arc<Mutex<OrderService>>;
```

- [ ] `Arc<Mutex<OrderService>>` ラッパーの型エイリアスと構築ヘルパーを追加
- [ ] `main.rs` での使用例を示すサンプルコードを追加
- [ ] マルチスレッド環境での動作確認テストを追加

**P4-4: 統合テスト・E2E テストの追加**

```
tests/
├── integration/
│   ├── order_lifecycle.rs      # submit → fill → pnl の統合テスト
│   ├── composite_orders.rs     # IFD / OCO の統合テスト
│   ├── margin_monitor.rs       # MarginCall → StopOut の統合テスト
│   └── ioc_fok.rs              # IOC / FOK の統合テスト
```

- [ ] `tests/integration/` ディレクトリを作成
- [ ] Limit 注文ライフサイクルの統合テストを追加
- [ ] 複合注文（IFD / OCO）の統合テストを追加
- [ ] MarginCall → StopOut → ポジションクローズの統合テストを追加
- [ ] IOC / FOK の統合テストを追加

---

## Risks and Mitigations

| リスク | 影響度 | 発生確率 | 対策 |
|---|:---:|:---:|---|
| sqlx 非同期化による `OrderService` API の大規模変更 | 高 | 中 | Phase 3 で同期 API を維持し、非同期は Phase 4 でオプション追加 |
| `CompositeOrderService` リファクタ時の既存テスト破壊 | 中 | 中 | P1-4 実施前に既存テストを統合テストとして `tests/` に移動してから着手 |
| SQL DDL 修正により既存 DB データが破損 | 中 | 低 | `000_drop_all.sql` は開発用途のみ。本番は `ALTER TABLE` のマイグレーションで対応 |
| IOC/FOK のシミュレーション意味論が実運用と乖離 | 低 | 高 | ドキュメントに「外部マッチングエンジン前提」の設計制約を明記 |
| 非同期 EventBus 導入で `EventHandler` トレイトが `async fn` 非対応 | 中 | 高 | `async_trait` クレートまたは `tokio::spawn` でハンドラを別タスク化 |

---

## Success Metrics

| フェーズ | 指標 |
|---|---|
| P1 完了 | `cargo test` 全パス / `cargo clippy -- -D warnings` 警告ゼロ / `main.rs` 50行以内 |
| P2 完了 | IOC/FOK テスト追加・全パス / 入出金 API テスト追加・全パス |
| P3 完了 | Docker Compose で PostgreSQL 起動・統合テスト全パス |
| P4 完了 | 非同期 EventBus で E2E テストパス / 設定ファイルで銘柄・リスク設定が読み込まれる |
| 全体 | テスト総数 120件以上 / ドキュメントが最新コードと一致 |

---

## Validation Strategy

### Phase 1
```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

### Phase 2
```bash
cargo test domain::account::tests::deposit_withdraw
cargo test service::order_service::tests::ioc_
cargo test service::order_service::tests::fok_
```

### Phase 3
```bash
docker compose up -d postgres
cargo test --features db-integration
```

### Phase 4
```bash
cargo test --all
# E2E: CLI コマンドの手動実行
cargo run -- repl
```

---

## Open Questions

1. **非同期化の範囲**: `OrderService` 全体を `async` にするか、`EventBus` だけ非同期にするか？　→ Phase 4 開始前に決定が必要
2. **Web API 対応**: REST API（axum / actix-web）を追加するか？　→ 現計画には含めていない。Phase 4 完了後の次フェーズとして別計画で扱う
3. **認証・マルチアカウント**: 複数口座対応が必要か？　→ 現実装は単一口座前提。追加が必要なら P3 の Repository 設計時に考慮
4. **IOC/FOK の意味論**: 外部マッチングエンジンが存在しないシミュレーション環境で IOC/FOK をどう模倣するか？　→ P2-1 設計時に明確化

---

## Quality Rubric Scores (Final)

| 項目 | スコア | 根拠 |
|---|:---:|---|
| **Completeness** | 5/5 | 既存実装の全ギャップを網羅し、4フェーズで全機能を対応 |
| **Feasibility** | 4/5 | 全タスクが単一開発者でも実行可能。非同期化のみ設計上のリスクあり |
| **Risk Coverage** | 4/5 | 主要リスク（API 変更・DB 破損・非同期化）を特定し対策を記載 |
| **Testability** | 5/5 | 各フェーズにテスト追加タスクを明示。統合テスト環境も計画 |
| **Maintainability** | 5/5 | 段階的フェーズ構成、依存関係の明示、設定外部化により保守性が向上 |

---

## Refinement Notes

**初稿から改善した点:**

1. **SQL DDL の具体的な乖離リストを追加** — 当初は「DDL と型が合っていない」とだけ記述していたが、乖離カラムを表で明示した
2. **`CompositeOrderService` のカプセル化修正をフェーズ 1 に繰り上げ** — 当初はフェーズ 3 に配置していたが、DB 接続前に解消しないと Repository トレイト設計に影響するため繰り上げた
3. **IOC/FOK のシミュレーション意味論リスクを明示** — 外部マッチングエンジン前提の設計上の限界を Open Questions と Risks に追記した
4. **統合テストディレクトリ構成を具体化** — `tests/integration/` の各ファイルと対象機能を明示した

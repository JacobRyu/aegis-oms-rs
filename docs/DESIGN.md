# Aegis OMS — 設計ドキュメント

## 1. プロジェクト概要

**Aegis OMS** は、FX および暗号資産（Crypto）向けの Order Management System (OMS) です。
Rust 製で、CLI/REPL で動作するミニ実装であり、ドメイン駆動設計（DDD）に基づくレイヤ分割と
`rust_decimal` を用いた正確な数値計算により、トレーディング・コア機能の学習・検証を目的としています。

---

## 2. アーキテクチャ設計

### 2.1 レイヤードアーキテクチャ

```
┌─────────────────────────────────────────┐
│              main.rs / cli/             │  プレゼンテーション層
├─────────────────────────────────────────┤
│           service/                      │  アプリケーション層
│  order_service.rs, risk_check.rs,       │
│  composite_order_service.rs,            │
│  margin_monitor.rs, shared.rs           │
├─────────────────────────────────────────┤
│           domain/                       │  ドメイン層
│  order.rs, account.rs, position.rs,     │
│  instrument.rs, event.rs, error.rs,     │
│  trade.rs, repository.rs,              │
│  composite_order.rs, risk_engine.rs     │
├─────────────────────────────────────────┤
│           infra/                        │  インフラストラクチャ層
│  order_store.rs, event_bus.rs,          │
│  pg_order_repo.rs, pg_account_repo.rs,  │
│  pg_trade_repo.rs, db.rs, alert.rs,     │
│  trade_store.rs, async_event_bus.rs     │
└─────────────────────────────────────────┘
```

#### なぜこのレイヤ分割なのか？

1. **ドメイン層の純粋性**: `domain/` は外部 crate にほぼ非依存です。これにより、ビジネスロジックを
   テストしやすく、インフラの変更から守ることができます。Rust の所有権モデルと組み合わせることで、
   ランタイムエラーをコンパイル時に防ぐ設計になっています。

2. **依存関係の単方向性**: `domain ← service ← infra ← main` の単方向依存により、
   上位レイヤが下位レイヤの実装细节を知らなくても済みます。Repository トレイトがこの境界を定義しています。

3. **インフラの交換可能性**: in-memory ストアと PostgreSQL リポジトリを切り替え可能にすることで、
   開発時の高速なテスト実行と本番環境での永続化の両方に対応できます。

### 2.2 Repository パターン

```rust
// src/domain/repository.rs
pub trait OrderRepository: Send {
    fn save(&mut self, order: Order) -> Result<()>;
    fn get(&self, id: &OrderId) -> Option<&Order>;
    fn get_mut(&mut self, id: &OrderId) -> Option<&mut Order>;
    fn find_open_orders(&self) -> Vec<&Order>;
    fn find_pending_trigger_orders(&self) -> Vec<&Order>;
    fn all_orders(&self) -> Vec<&Order>;
    fn load_all_owned(&self) -> Result<Vec<Order>>;
}

pub trait TradeRepository: Send {
    fn save(&mut self, trade: Trade) -> Result<()>;
    fn all(&self) -> Vec<&Trade>;
    fn by_instrument(&self, symbol: &str) -> Vec<&Trade>;
    fn load_all_owned(&self) -> Result<Vec<Trade>>;
}

pub trait AccountRepository: Send {
    fn save(&mut self, account: &Account) -> Result<()>;
    fn load(&self, id: &str) -> Result<Option<Account>>;
}
```

#### なぜ Repository パターンなのか？

- **ドメイン層とインフラ層の分離**: ドメインロジックが具体的な永続化技術を知らなくて済む
- **テスト容易性**: テスト時は InMemoryOrderStore、本番は PgOrderRepository を注入可能
- **将来拡張性**: Redis や DynamoDB への切り替えが必要になっても、サービス層を変更不要

---

## 3. ドメインモデル設計

### 3.1 注文（Order）

```rust
pub struct Order {
    pub id: OrderId,           // ULID (時系列ソート可能なUUID)
    pub instrument: String,     // 銘柄シンボル
    pub side: Side,             // Buy / Sell
    pub order_type: OrderType,  // Market / Limit / Stop / StopLimit / TrailingStop
    pub quantity: Decimal,      // 注文数量
    pub filled_quantity: Decimal, // 約定済み数量
    pub time_in_force: TimeInForce, // GTC / IOC / FOK
    pub status: OrderStatus,    // 注文ステータス
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub best_price: Option<Decimal>, // TrailingStop 専用
}
```

#### なぜ OrderType を enum で表現したのか？

```rust
pub enum OrderType {
    Market,
    Limit { price: Decimal },
    Stop { trigger_price: Decimal },
    StopLimit { trigger_price: Decimal, limit_price: Decimal },
    TrailingStop { trail_amount: Decimal },
}
```

- **型安全性**: 各注文種別に固有のパラメータを持たせることで、不正な状態をコンパイル時に防止
- **パターンマッチング**: `match` による分岐で、注文種別ごとの処理を明確に記述可能
- **拡張性**: 新しい注文種別（例: Iceberg Order）を追加する際、既存コードの影響範囲が限定される

### 3.2 注文ステートマシン

```
New → PendingTrigger → Accepted → PartiallyFilled → Filled
  ↘       ↓               ↓            ↓
  Rejected  Cancelled     Cancelled    Cancelled
```

#### なぜステートマシンとして実装したのか？

- **不正遷移の防止**: `can_transition_to` メソッドで許可された遷移のみを定義
- **ビジネスルールの明示化**: どの状態からどの状態に遷移できるかがコード上で明確
- **監査可能性**: ステート遷移のログを取ることで、取引の監査証跡を残せる

```rust
pub fn can_transition_to(&self, next: OrderStatus) -> bool {
    matches!(
        (self, next),
        (OrderStatus::New, OrderStatus::PendingTrigger)
            | (OrderStatus::New, OrderStatus::Accepted)
            | (OrderStatus::New, OrderStatus::Rejected)
            | (OrderStatus::PendingTrigger, OrderStatus::Accepted)
            | (OrderStatus::PendingTrigger, OrderStatus::Cancelled)
            | (OrderStatus::Accepted, OrderStatus::PartiallyFilled)
            | (OrderStatus::Accepted, OrderStatus::Filled)
            | (OrderStatus::Accepted, OrderStatus::Cancelled)
            | (OrderStatus::PartiallyFilled, OrderStatus::PartiallyFilled)
            | (OrderStatus::PartiallyFilled, OrderStatus::Filled)
            | (OrderStatus::PartiallyFilled, OrderStatus::Cancelled)
    )
}
```

### 3.3 口座（Account）

```rust
pub struct Account {
    pub id: AccountId,
    pub name: String,
    pub balance: Decimal,        // 現在残高
    pub locked_margin: Decimal,  // ロック中の証拠金
    pub created_at: DateTime<Utc>,
}
```

#### なぜ balance と locked_margin を分離したのか？

- **レバレッジ取引の本質を表現**: 証拠金取引では、口座残高から証拠金をロックし、
  利用可能残高を計算する必要がある
- **整合性の保証**: `available_balance() = balance - locked_margin` という不変条件を维护
- **ロック解除の安全性**: `unlock_margin` でロック超過を検出し、不正な状態を防止

### 3.4 ポジション（Position）

```rust
pub struct Position {
    pub instrument: String,
    pub side: Side,
    pub quantity: Decimal,
    pub avg_price: Decimal,      // 加重平均建値
    pub unrealized_pnl: Decimal, // 含み損益
}
```

#### なぜネッティング方式を採用したのか？

- **FX/Crypto の一般的な慣習**: 多くの取引所がネッティング方式を採用
- **メモリ効率**: 同一銘柄のポジションを1つに集約することで、メモリ使用量を削減
- **加重平均の計算**: `add` メソッドで同方向の約定を加重平均、`reduce` で反対方向の約定を適用

### 3.5 銘柄（Instrument）

```rust
pub struct Instrument {
    pub symbol: String,
    pub asset_class: AssetClass, // Fx / Crypto
    pub tick_size: Decimal,      // 最小価格変動幅
    pub lot_size: Decimal,       // 最小取引単位
    pub leverage: Decimal,       // レバレッジ倍率
}
```

#### なぜ銘柄マスタをドメインに持たせたのか？

- **取引ルールの集中管理**: tick_size, lot_size, leverage は取引に不可欠なパラメータ
- **バリデーションの統一**: 注文受付時に銘柄の制約を一元的にチェック可能
- **拡張性**: 将来的に銘柄ごとの取引時間を追加する際の拡張ポイント

---

## 4. サービス層設計

### 4.1 OrderService（注文サービス）

```rust
pub struct OrderService {
    pub store: Box<dyn OrderRepository>,
    pub account: Account,
    pub instruments: HashMap<String, Instrument>,
    pub positions: HashMap<String, Position>,
    margin_locks: HashMap<OrderId, Decimal>,
    risk_checker: RiskChecker,
    pub event_bus: EventBus,
    trade_store: Box<dyn TradeRepository>,
    cumulative_realized_loss: Decimal,
}
```

#### なぜ OrderService が这么大的 Responsibility を持つのか？

- **Facade パターン**: 注文ライフサイクルの複雑な処理を隠蔽し、シンプルな API を提供
- **アトミックな状態変更**: 証拠金ロック、ポジション更新、イベント発行を一つのトランザクションで実行
- **整合性の保証**: 複数のドメインオブジェクトをまたぐ操作を一括で管理

### 4.2 リスクチェック

```rust
pub struct RiskLimits {
    pub max_order_quantity: Decimal,
    pub max_open_orders: usize,
    pub stop_out_ratio: Decimal,
    pub margin_call_ratio: Decimal,
    pub max_open_positions: usize,
    pub max_loss: Option<Decimal>,
}
```

#### なぜリスクチェックを独立させたのか？

- **単一責任原則**: リスク管理ロジックを注文処理から分離
- **設定の柔軟性**: RiskLimits を変更することで、取引ルールを動的に変更可能
- **テスト容易性**: リスクチェックロジックを単体でテスト可能

### 4.3 IOC/FOK 執行ポリシー

```rust
// IOC: Immediate or Cancel
// 提出時に即座に未約定残量を自動キャンセル
fn apply_ioc_policy(&mut self, order_id: OrderId) -> Result<()> {
    let residual = order.remaining_quantity();
    if residual > Decimal::ZERO {
        order.cancel()?;
        // 証拠金ロック解除
    }
    Ok(())
}

// FOK: Fill or Kill
// 全量約定不能なら即座にリジェクト
if req.time_in_force == TimeInForce::FOK {
    order.reject()?;
    return Err(OmsError::FokRejected { ... });
}
```

#### なぜ IOC/FOK をこのタイミングで処理したのか？

- **現実の取引所の動作に忠実**: IOC/FOK は提出時に即座に判断される
- **証拠金の効率利用**: 未約定残量を早期に解放することで、資金の拘束を最小化
- **イベント発行**: 残量キャンセルを `IocResidualCancelled` イベントとして通知

---

## 5. インフラ層設計

### 5.1 EventBus

```rust
pub trait EventHandler: Send + Sync {
    fn handle(&self, event: &OrderEvent);
}

pub struct EventBus {
    handlers: Vec<Box<dyn EventHandler>>,
}
```

#### なぜ同期的な EventBus を採用したのか？

- **シンプルさ**: 非同期化は将来的に `async_event_bus.rs` として提供
- **确定性**: 同期処理により、イベントハンドラの実行順序が明確
- **テスト容易性**: テスト内でイベントの発行を容易に検証可能

### 5.2 PostgreSQL リポジトリ

```rust
// src/infra/pg_order_repo.rs
pub struct PgOrderRepository {
    pool: PgPool,
}
```

#### なぜ sqlx を採用したのか？

- **コンパイル時クエリチェック**: `sqlx::query!` マクロにより、SQL の構文エラーをコンパイル時に検出
- **型安全**: Rust の型と PostgreSQL の型を自動的にマッピング
- **パフォーマンス**: 非同期 I/O とコネクションプールによる高パフォーマンス

### 5.3 データベーススキーマ設計

```sql
-- 注文テーブル
CREATE TABLE orders (
    id              TEXT PRIMARY KEY,  -- ULID
    account_id      TEXT NOT NULL REFERENCES accounts(id),
    instrument      TEXT NOT NULL REFERENCES instruments(symbol),
    side            order_side NOT NULL,
    order_type      order_type NOT NULL,
    price           NUMERIC,           -- Market 注文は NULL
    quantity        NUMERIC NOT NULL CHECK (quantity > 0),
    filled_quantity NUMERIC NOT NULL DEFAULT 0 CHECK (filled_quantity <= quantity),
    time_in_force   time_in_force NOT NULL DEFAULT 'gtc',
    status          order_status NOT NULL DEFAULT 'new',
    trigger_price   NUMERIC,           -- Stop/StopLimit のトリガー価格
    limit_price     NUMERIC,           -- StopLimit の指値価格
    trail_amount    NUMERIC,           -- TrailingStop のトレール幅
    best_price      NUMERIC,           -- TrailingStop 追従中の最良価格
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

#### なぜこのスキーマ設計なのか？

1. **NULL の活用**: Market 注文は `price` が NULL、Stop 注文は `trigger_price` が NULL
   → 不正な組み合わせを制約で防止

2. **部分インデックス**: `WHERE status NOT IN ('filled', 'cancelled', 'rejected')`
   → アクティブな注文のみをインデックスで高速検索

3. **ENUM 型の採用**: PostgreSQL の ENUM 型により、不正な値の挿入をデータベースレベルで防止

4. **CHECK 制約**: `filled_quantity <= quantity` により、オーバーフィルをデータベースレベルで防止

5. **updated_at トリガー**: アプリケーションコードを汚さずに、更新時刻を自動管理

---

## 6. 数値計算設計

### 6.1 rust_decimal の採用

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

// 正確な10進演算
let margin = MarginCalculator::required_margin(dec!(65000), dec!(1.0), dec!(2));
// margin = 32500 (正確)
```

#### なぜ f64 ではなく rust_decimal なのか？

- **金融計算の正確性**: f64 は浮動小数点誤差が蓄積し、金融計算では致命的な問題を引き起こす
- **IEEE 754 の限界**: `0.1 + 0.2 ≠ 0.3` という問題が、取引において資金の不整合を引き起こす
- **serde 対応**: JSON シリアライズ/デシリアライズが容易
- **PostgreSQL NUMERIC 型との互換性**: sqlx との統合が容易

### 6.2 加重平均建値の計算

```rust
// 同方向の約定: 加重平均を更新
pub fn add(&mut self, qty: Decimal, price: Decimal) {
    let total_cost = self.avg_price * self.quantity + price * qty;
    self.quantity += qty;
    self.avg_price = total_cost / self.quantity;
}

// 反対方向の約定: 実現損益を計算
pub fn reduce(&mut self, qty: Decimal, price: Decimal) -> Decimal {
    let close_qty = qty.min(self.quantity);
    let pnl = match self.side {
        Side::Buy => (price - self.avg_price) * close_qty,
        Side::Sell => (self.avg_price - price) * close_qty,
    };
    self.quantity -= close_qty;
    pnl
}
```

---

## 7. エラーハンドリング設計

### 7.1 thiserror の採用

```rust
#[derive(Debug, Error)]
pub enum OmsError {
    #[error("Invalid state transition for order {order_id}: {from} → {to}")]
    InvalidStateTransition { order_id: OrderId, from: OrderStatus, to: OrderStatus },

    #[error("Over-fill for order {order_id}: requested {requested}, remaining {remaining}")]
    OverFill { order_id: OrderId, requested: Decimal, remaining: Decimal },

    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: Decimal, available: Decimal },

    // ... 他のエラー variant
}
```

#### なぜ enum でエラーを定義したのか？

- **型安全性**: エラーの種別がコンパイル時に保証される
- **パターンマッチング**: エラーごとに異なる処理が可能
- **人間が読めるメッセージ**: `#[error(...)]` アトリビュートでエラーメッセージを定義

---

## 8. 設計上の意思決定とその正当性

### 8.1 ULID の採用（UUID v4 ではなく）

| 観点 | ULID | UUID v4 |
|------|------|---------|
| 時系列ソート | ○ | × |
| 分散生成 | ○ | ○ |
| 暗号学的安全性 | ○ | ○ |
| URL フレンドリー | ○ | × |

**理由**: 注文 ID は時系列ソートが必要（取引履歴の表示順序）。ULID は entropy 部分に
ランダム値を持ちつつ、タイムスタンプ部分で時系列ソートが可能。

### 8.2 serde の採用

- **JSON/API 対応**: 将来的な REST API 拡張への準備
- **ログ出力**: 構造化ログ（tracing）との親和性
- **永続化**: PostgreSQL の JSONB カラムとの親和性

### 8.3 thiserror vs anyhow

- **thiserror を採用**: ライブラリ的性質のプロジェクトため、明示的なエラー型を定義
- **anyhow は未採用**: アプリケーション層で使うことは可能だが、ドメインエラーは型安全であるべき

---

## 9. テスト設計

### 9.1 ユニットテスト戦略

各ドメインモジュールに `#[cfg(test)] mod tests` を配置：
- **ドメインロジックの検証**: 状態遷移、数値計算、ビジネスルール
- **エッジケースのカバー**: オーバーフィル、マージン不足、不正遷移
- **pretty_assertions**: 視覚的な差分表示でテスト失敗の原因を容易に特定

### 9.2 テストカバレッジの重点領域

1. **Order::transition_to**: 不正な状態遷移の防止
2. **Position::add/reduce**: 加重平均の正確性
3. **Account::lock_margin/unlock_margin**: 証拠金の整合性
4. **RiskChecker**: リスク制限の検証
5. **IOC/FOK ポリシー**: 執行ポリシーの正確な実装

---

## 10. 将来拡張への考慮

### 10.1 非同期イベントバス

`async_event_bus.rs` が既に存在し、将来的な非同期処理への移行が可能。

### 10.2 複合注文（Composite Order）

`composite_order.rs` / `composite_order_service.rs` により、
OCO（One Cancels Other）や Iceberg Order などの複合注文への拡張が準備されている。

### 10.3 マージンモニタリング

`margin_monitor.rs` により、 Margin Call / Stop Out の監視が実装されている。

### 10.4 アラートシステム

`alert.rs` および `SmtpConfig` により、メールアラートへの拡張が可能。

---

## 11. 開発環境設定

### 11.1 なぜ TOML 設定を採用したのか？

- **Rust の標準**: Cargo.toml と同じフォーマットで、開発者の認知負荷を軽減
- **階層的な設定**: `config/default.toml` + 環境変数（`AEGIS_CONFIG`）で柔軟な設定が可能
- **型安全なデシリアライズ**: serde + toml crate で、設定のパースエラーをコンパイル時に検出

### 11.2 pre-commit の採用

- **品質保証**: コミット前に lint、テスト、型チェックを自動実行
- **チーム開発**: コーディング規約の自動適用

---

## 12. まとめ

Aegis OMS の設計は、以下の原則に基づいています：

1. **ドメイン駆動設計**: ビジネスロジックをコアに据え、インフラやUI から分離
2. **型安全**: Rust の型システムを活用し、不正な状態をコンパイル時に防止
3. **テスト容易性**: 各レイヤが独立してテスト可能
4. **将来拡張性**: Repository パターン、イベントバス、非同期処理への移行が容易
5. **金融計算の正確性**: rust_decimal による10進演算で浮動小数点誤差を排除

これらの設計判断により、学習・検証用のミニ実装でありながら、
本番品質のアーキテクチャ持有しています。

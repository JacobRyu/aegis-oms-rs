# Aegis OMS — システム概要

## 1. プロジェクト概要

**Aegis OMS** は FX・暗号資産（Crypto）向けの軽量な **Order Management System (OMS)** です。
Rust 製の CLI / REPL として動作し、ドメイン駆動設計（DDD）に基づくレイヤ分割で実装されています。
学習・検証用のリファレンス実装として、トレーディングコアの主要概念を網羅しています。

---

## 2. アーキテクチャ概要

### レイヤ構成

```
┌──────────────────────────────────────────────────────────┐
│  Presentation (main.rs)                                  │
│  CLI / REPL — clap コマンドパーサ                         │
└───────────────────────┬──────────────────────────────────┘
                        │ calls
┌───────────────────────▼──────────────────────────────────┐
│  Service (service/)                                      │
│  OrderService          — 注文ライフサイクル統合管理          │
│  CompositeOrderService — IFD/OCO/Stop トリガー管理         │
│  MarginMonitor         — 証拠金監視・強制ロスカット          │
│  RiskChecker           — リスクバリデーション               │
└──────┬────────────────────────────────────┬──────────────┘
       │ uses domain models                 │ calls
┌──────▼──────────────────┐      ┌──────────▼──────────────┐
│  Domain (domain/)       │      │  Infra (infra/)         │
│  Order / OrderType      │      │  InMemoryOrderStore     │
│  Position               │      │  InMemoryTradeStore     │
│  Account                │      │  EventBus               │
│  Instrument             │      │  AlertSender            │
│  Trade                  │      │  (Console / SMTP)       │
│  IfdOrder / OcoOrder    │      └─────────────────────────┘
│  MarginStatus / Risk    │
│  OmsError / OrderEvent  │
└─────────────────────────┘
```

### 依存方向

```
domain ← service ← infra ← main (presentation)
```

ドメイン層は外部 crate に対しほぼ非依存（`rust_decimal`, `chrono`, `serde`, `ulid` のみ）。

---

## 3. データモデル

### Account（口座）

| フィールド          | 型          | 説明                           |
|:----------------|:------------|:------------------------------|
| `id`            | `String`    | 口座 ID                        |
| `name`          | `String`    | 口座名                         |
| `balance`       | `Decimal`   | 残高（入出金 + 実現損益を反映）   |
| `locked_margin` | `Decimal`   | 注文中のロック証拠金             |
| `created_at`    | `DateTime`  | 作成日時                       |

- **利用可能残高** = `balance - locked_margin`

### Instrument（銘柄）

| フィールド        | 型           | 説明                |
|:--------------|:-------------|:-------------------|
| `symbol`      | `String`     | 銘柄シンボル         |
| `asset_class` | `AssetClass` | `Fx` / `Crypto`    |
| `tick_size`   | `Decimal`    | 最小価格変動幅        |
| `lot_size`    | `Decimal`    | 最小取引単位         |
| `leverage`    | `Decimal`    | レバレッジ倍率        |

### Order（注文）

| フィールド            | 型              | 説明                                                         |
|:-----------------|:----------------|:------------------------------------------------------------|
| `id`             | `OrderId(Ulid)` | ULID ベースの注文 ID                                          |
| `instrument`     | `String`        | 銘柄シンボル                                                  |
| `side`           | `Side`          | `Buy` / `Sell`                                              |
| `order_type`     | `OrderType`     | `Market` / `Limit` / `Stop` / `StopLimit` / `TrailingStop`  |
| `quantity`       | `Decimal`       | 発注数量                                                     |
| `filled_quantity`| `Decimal`       | 約定済み数量                                                  |
| `time_in_force`  | `TimeInForce`   | `GTC` / `IOC` / `FOK`                                       |
| `status`         | `OrderStatus`   | 注文ステータス                                                |
| `best_price`     | `Option<Decimal>` | TrailingStop が追従する最良価格                              |

#### OrderType バリアント

| バリアント                                          | 説明                                                  |
|:--------------------------------------------------|:----------------------------------------------------|
| `Market`                                          | 成行注文。発注即受付、約定時に証拠金チェック              |
| `Limit { price }`                                 | 指値注文。発注時に証拠金ロック                           |
| `Stop { trigger_price }`                          | 逆指値。trigger_price 到達で Market として執行           |
| `StopLimit { trigger_price, limit_price }`        | 逆指値指値。trigger_price 到達で Limit として発注        |
| `TrailingStop { trail_amount }`                   | トレーリングストップ。best_price から trail_amount 乖離で執行 |

### Position（ポジション）

| フィールド           | 型        | 説明                                           |
|:----------------|:----------|:----------------------------------------------|
| `instrument`    | `String`  | 銘柄シンボル                                    |
| `side`          | `Side`    | `Buy`（ロング） / `Sell`（ショート）              |
| `quantity`      | `Decimal` | 保有数量                                       |
| `avg_price`     | `Decimal` | 加重平均建値                                    |
| `unrealized_pnl`| `Decimal` | 未実現損益（`update_positions_pnl` で更新）       |

### Trade（約定履歴）

| フィールド          | 型                  | 説明                         |
|:----------------|:--------------------|:----------------------------|
| `id`            | `TradeId(Ulid)`     | 約定 ID                      |
| `order_id`      | `OrderId`           | 対応する注文 ID               |
| `instrument`    | `String`            | 銘柄シンボル                  |
| `side`          | `Side`              | `Buy` / `Sell`              |
| `quantity`      | `Decimal`           | 約定数量                     |
| `price`         | `Decimal`           | 約定価格                     |
| `realized_pnl`  | `Option<Decimal>`   | 実現損益（クローズ時のみ）       |
| `executed_at`   | `DateTime<Utc>`     | 約定日時                     |

### MarginStatus（証拠金ステータス）

| フィールド              | 型           | 説明                              |
|:--------------------|:-------------|:---------------------------------|
| `equity`            | `Decimal`    | 純資産 = balance + unrealized_pnl |
| `used_margin`       | `Decimal`    | 使用中証拠金（全ポジション合計）     |
| `free_margin`       | `Decimal`    | 余剰証拠金 = equity − used_margin  |
| `margin_ratio`      | `Decimal`    | 証拠金率 (%) = equity/used×100    |
| `effective_leverage`| `Decimal`    | 実効レバレッジ = 総想定元本/equity  |
| `level`             | `MarginLevel`| `Normal` / `MarginCall` / `StopOut` |

### IfdOrder / OcoOrder（複合注文）

| 型           | フィールド                               | 説明                                      |
|:-----------|:---------------------------------------|:------------------------------------------|
| `IfdOrder` | `primary_id`, `secondary_req`          | 親約定後に子注文を自動発注                   |
| `OcoOrder` | `order_a_id`, `order_b_id`             | 片方約定でもう一方を自動キャンセル            |

---

## 4. デフォルト銘柄マスタ

| Symbol  | Asset Class | Tick Size | Lot Size | Leverage |
|:--------|:------------|----------:|---------:|---------:|
| USD/JPY | FX          |     0.001 |     1000 |      x25 |
| EUR/USD | FX          |   0.00001 |     1000 |      x25 |
| BTC/USD | Crypto      |      0.01 |    0.001 |       x2 |
| ETH/USD | Crypto      |      0.01 |     0.01 |       x2 |

初期口座残高: **100,000**（`acc-001` / "Default"）

---

## 5. 主要コンポーネント説明

### OrderService (`service/order_service.rs`)

注文ライフサイクルを統合管理するアプリケーションサービス。

| メソッド                       | 説明                                |
|:----------------------------|:-----------------------------------|
| `submit_order`              | 注文の受付・リスクチェック・証拠金ロック   |
| `cancel_order`              | 注文キャンセル・証拠金ロック解除         |
| `fill_order`                | 約定適用・証拠金管理・ポジション更新      |
| `update_positions_pnl`      | 市場価格マップを受け取り未実現損益を更新   |
| `get_trade_history`         | 約定履歴照会（銘柄フィルタ対応）          |
| `get_order` / `get_open_orders` | 注文照会                          |
| `get_positions`             | ポジション照会                        |
| `get_account`               | 口座情報照会                         |

### CompositeOrderService (`service/composite_order_service.rs`)

IFD・OCO・Stop 系注文を横断管理するサービス。

| メソッド              | 説明                                                   |
|:------------------|:------------------------------------------------------|
| `register_ifd`    | IFD 注文を登録（親注文 ID + 子注文リクエスト）              |
| `register_oco`    | OCO 注文を登録（2つの注文 ID）                            |
| `on_price_update` | 価格更新通知。Stop/TrailingStop のトリガー判定を実行         |
| `on_order_filled` | 約定通知。IFD 子発注・OCO 相手キャンセルを自動実行           |

### MarginMonitor (`service/margin_monitor.rs`)

証拠金率をリアルタイムに監視し、アラート送信・強制ロスカットを行うサービス。

| メソッド       | 説明                                                              |
|:------------|:-----------------------------------------------------------------|
| `check`     | MarginStatus を計算。MarginCall→アラート、StopOut→強制ロスカット発動 |

### RiskChecker (`service/risk_check.rs`)

| チェック項目               | デフォルト値         |
|:----------------------|:-----------------|
| 数量バリデーション（ゼロ・負・超過） | max 1,000,000    |
| 価格バリデーション（Limit/Stop 系） | 正値のみ          |
| 未約定注文数上限              | 100 件           |
| 証拠金バリデーション            | available ≥ required |
| ロスカット発動水準             | 50%              |
| 追証アラート発動水準            | 100%             |

**必要証拠金の計算式：**
```
required_margin = (price × quantity) / leverage
```

### AlertSender (`infra/alert.rs`)

| 実装クラス              | 説明                                               |
|:--------------------|:-------------------------------------------------|
| `ConsoleAlertSender` | 標準エラー出力。デフォルト・テスト用               |
| `SmtpAlertSender`   | lettre 経由の SMTP メール送信。資格情報は環境変数から |

**環境変数（SmtpAlertSender）：**

| 変数名       | 説明                      |
|:-----------|:------------------------|
| `SMTP_HOST` | SMTP サーバーホスト名       |
| `SMTP_PORT` | ポート番号（デフォルト 587） |
| `SMTP_USER` | ユーザー名               |
| `SMTP_PASS` | パスワード               |
| `ALERT_FROM`| 送信元アドレス            |
| `ALERT_TO`  | 送信先アドレス            |

### InMemoryOrderStore / InMemoryTradeStore (`infra/`)

| ストア                  | 説明                                      |
|:---------------------|:-----------------------------------------|
| `InMemoryOrderStore` | `HashMap<OrderId, Order>` によるインメモリ注文ストア |
| `InMemoryTradeStore` | `Vec<Trade>` による約定履歴ストア（新しい順で返却） |

### EventBus (`infra/event_bus.rs`)

注文ライフサイクルのドメインイベントを登録済みハンドラへ通知するシンプルなパブサブ実装。

| イベント                  | 発生タイミング              |
|:-----------------------|:------------------------|
| `Accepted`             | 注文受付時（Limit/Market） |
| `PartiallyFilled`      | 部分約定時                |
| `Filled`               | 完全約定時                |
| `Cancelled`            | キャンセル時              |
| `Rejected`             | リスクチェック失敗時        |
| `StopTriggered`        | Stop/TrailingStop トリガー時 |
| `MarginCall`           | 追証水準到達時             |
| `StopOut`              | ロスカット水準到達時         |
| `TrailingStopUpdated`  | TrailingStop 最良価格更新時  |

---

## 6. エラー体系 (`domain/error.rs`)

| エラー種別                   | 発生条件                             |
|:--------------------------|:------------------------------------|
| `InvalidStateTransition`  | 不正な注文ステータス遷移              |
| `OverFill`                | 残数量を超えた約定                   |
| `OrderNotFound`           | 存在しない注文 ID                    |
| `InsufficientFunds`       | 利用可能残高 < 必要証拠金            |
| `InvalidMarginAmount`     | ゼロ・負の証拠金額                   |
| `MarginUnlockExceeded`    | ロック額を超えたアンロック要求         |
| `RiskCheckFailed`         | 数量・価格・注文数上限違反            |
| `InstrumentNotFound`      | 未登録の銘柄シンボル                  |
| `CompositeOrderNotFound`  | 存在しない複合注文 ID                |
| `AlertFailed`             | アラート送信失敗（SMTP エラー等）      |


## 1. プロジェクト概要

**Aegis OMS** は FX・暗号資産（Crypto）向けの軽量な **Order Management System (OMS)** です。
Rust 製の CLI / REPL として動作し、ドメイン駆動設計（DDD）に基づくレイヤ分割で実装されています。
学習・検証用のリファレンス実装として、トレーディングコアの主要概念を網羅しています。

---

## 2. アーキテクチャ概要

### レイヤ構成

```
┌─────────────────────────────────────────────┐
│  Presentation (main.rs)                     │
│  CLI / REPL — clap コマンドパーサ            │
└──────────────┬──────────────────────────────┘
               │ calls
┌──────────────▼──────────────────────────────┐
│  Service (service/)                         │
│  OrderService — 注文ライフサイクル統合管理    │
│  RiskChecker  — リスクバリデーション          │
└──────┬───────────────────────────┬──────────┘
       │ uses domain models        │ calls
┌──────▼──────┐          ┌─────────▼─────────┐
│  Domain     │          │  Infra            │
│  (domain/)  │          │  (infra/)         │
│  Order      │          │  InMemoryOrderStore│
│  Position   │          │  EventBus         │
│  Account    │          └───────────────────┘
│  Instrument │
│  OmsError   │
│  OrderEvent │
└─────────────┘
```

### 依存方向

```
domain ← service ← infra ← main (presentation)
```

ドメイン層は外部 crate に対しほぼ非依存（`rust_decimal`, `chrono`, `serde`, `ulid` のみ）。

---

## 3. データモデル

### Account（口座）

| フィールド       | 型          | 説明                           |
|:-------------|:------------|:------------------------------|
| `id`         | `String`    | 口座 ID                        |
| `name`       | `String`    | 口座名                         |
| `balance`    | `Decimal`   | 残高（入出金 + 実現損益を反映）   |
| `locked_margin` | `Decimal` | 注文中のロック証拠金            |
| `created_at` | `DateTime`  | 作成日時                       |

- **利用可能残高** = `balance - locked_margin`

### Instrument（銘柄）

| フィールド      | 型           | 説明                |
|:------------|:-------------|:-------------------|
| `symbol`    | `String`     | 銘柄シンボル         |
| `asset_class` | `AssetClass` | `Fx` / `Crypto`  |
| `tick_size` | `Decimal`    | 最小価格変動幅        |
| `lot_size`  | `Decimal`    | 最小取引単位         |
| `leverage`  | `Decimal`    | レバレッジ倍率        |

### Order（注文）

| フィールド          | 型             | 説明                         |
|:----------------|:---------------|:----------------------------|
| `id`            | `OrderId(Ulid)` | ULID ベースの注文 ID         |
| `instrument`    | `String`       | 銘柄シンボル                  |
| `side`          | `Side`         | `Buy` / `Sell`              |
| `order_type`    | `OrderType`    | `Market` / `Limit { price }` |
| `quantity`      | `Decimal`      | 発注数量                     |
| `filled_quantity` | `Decimal`    | 約定済み数量                  |
| `time_in_force` | `TimeInForce`  | `GTC` / `IOC` / `FOK`       |
| `status`        | `OrderStatus`  | 注文ステータス                 |

### Position（ポジション）

| フィールド          | 型        | 説明                                        |
|:----------------|:----------|:------------------------------------------|
| `instrument`    | `String`  | 銘柄シンボル                                |
| `side`          | `Side`    | `Buy`（ロング） / `Sell`（ショート）          |
| `quantity`      | `Decimal` | 保有数量                                   |
| `avg_price`     | `Decimal` | 加重平均建値                                |
| `unrealized_pnl` | `Decimal` | 未実現損益（`update_positions_pnl` で更新）  |

---

## 4. デフォルト銘柄マスタ

| Symbol  | Asset Class | Tick Size | Lot Size | Leverage |
|:--------|:------------|----------:|---------:|---------:|
| USD/JPY | FX          |     0.001 |     1000 |      x25 |
| EUR/USD | FX          |   0.00001 |     1000 |      x25 |
| BTC/USD | Crypto      |      0.01 |    0.001 |       x2 |
| ETH/USD | Crypto      |      0.01 |     0.01 |       x2 |

初期口座残高: **100,000**（`acc-001` / "Default"）

---

## 5. 主要コンポーネント説明

### OrderService (`service/order_service.rs`)

注文ライフサイクルを統合管理するアプリケーションサービス。

| メソッド                     | 説明                              |
|:--------------------------|:---------------------------------|
| `submit_order`            | 注文の受付・リスクチェック・証拠金ロック |
| `cancel_order`            | 注文キャンセル・証拠金ロック解除      |
| `fill_order`              | 約定適用・証拠金管理・ポジション更新   |
| `update_positions_pnl`   | 市場価格を受け取り未実現損益を更新     |
| `get_order` / `get_open_orders` | 注文照会                    |
| `get_positions`           | ポジション照会                     |
| `get_account`             | 口座情報照会                       |

### RiskChecker (`service/risk_check.rs`)

| チェック項目         | 内容                               |
|:-----------------|:----------------------------------|
| 数量バリデーション   | ゼロ・負・最大数量超過              |
| 価格バリデーション   | Limit 注文の価格が正値であること      |
| 未約定注文数上限     | デフォルト 100 件                   |
| 証拠金バリデーション | `available_balance ≥ required_margin` |

**必要証拠金の計算式：**
```
required_margin = (price × quantity) / leverage
```

### InMemoryOrderStore (`infra/order_store.rs`)

`HashMap<OrderId, Order>` によるインメモリ注文ストア。

### EventBus (`infra/event_bus.rs`)

注文ライフサイクルのドメインイベントを登録済みハンドラへ通知するシンプルなパブサブ実装。

| イベント             | 発生タイミング         |
|:------------------|:-------------------|
| `Accepted`        | 注文受付時            |
| `PartiallyFilled` | 部分約定時            |
| `Filled`          | 完全約定時            |
| `Cancelled`       | キャンセル時          |
| `Rejected`        | リスクチェック失敗時   |

---

## 6. エラー体系 (`domain/error.rs`)

| エラー種別                   | 発生条件                             |
|:--------------------------|:------------------------------------|
| `InvalidStateTransition`  | 不正な注文ステータス遷移              |
| `OverFill`                | 残数量を超えた約定                   |
| `OrderNotFound`           | 存在しない注文 ID                    |
| `InsufficientFunds`       | 利用可能残高 < 必要証拠金            |
| `InvalidMarginAmount`     | ゼロ・負の証拠金額                   |
| `MarginUnlockExceeded`    | ロック額を超えたアンロック要求         |
| `RiskCheckFailed`         | 数量・価格・注文数上限違反            |
| `InstrumentNotFound`      | 未登録の銘柄シンボル                  |

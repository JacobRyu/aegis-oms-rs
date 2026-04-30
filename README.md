# Aegis OMS

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-2024-orange.svg)](https://www.rust-lang.org/)

**Aegis OMS** は、FX および 暗号資産（Crypto）向けに設計された、Rust 製の軽量な
**Order Management System (OMS)** です。CLI / REPL で動作するミニ実装で、
ドメイン駆動設計（DDD）に基づくレイヤ分割と、`rust_decimal` を用いた
正確な数値計算により、トレーディング・コア機能の学習・検証に適しています。

---

## ✨ 主な機能

- 📈 **注文管理**: Market / Limit 注文の発注、キャンセル、部分約定
- 💼 **ポジション管理**: 平均建値・含み損益（Unrealized PnL）の自動計算
- 💰 **証拠金 / 口座管理**: レバレッジを考慮した必要証拠金の計算とロック
- 🛡 **リスクチェック**: 最大注文数量・最大未約定注文数などの事前バリデーション
- 🔔 **イベントバス**: 注文ライフサイクルのドメインイベント発行
- 🖥 **CLI / REPL**: ワンショットコマンドと対話モードの両対応
- 🗄 **PostgreSQL DDL**: 永続化に向けたスキーマ定義（`sql/`）

---

## 🏗 アーキテクチャ

```
src/
├── domain/        # ドメイン層（純粋なビジネスロジック）
│   ├── account.rs     # 口座 / 証拠金計算
│   ├── instrument.rs  # 銘柄マスタ（FX / Crypto）
│   ├── order.rs       # 注文・Side・OrderType・Status
│   ├── position.rs    # ポジション・PnL
│   ├── event.rs       # ドメインイベント
│   └── error.rs       # OmsError
│
├── service/       # アプリケーション層（ユースケース）
│   ├── order_service.rs  # 発注・キャンセル・約定処理
│   └── risk_check.rs     # リスクリミット
│
├── infra/         # インフラ層
│   ├── order_store.rs    # 注文ストア（in-memory）
│   └── event_bus.rs      # イベントバス
│
├── lib.rs
└── main.rs        # CLI / REPL エントリポイント
```

依存関係は `domain ← service ← infra ← main` の単方向で、
ドメイン層は外部 crate にほぼ非依存です。

---

## 🚀 クイックスタート

### 必要環境
- Rust **2024 edition** 対応の toolchain（推奨: `rustup` 経由の最新 stable）

### ビルド

```bash
cargo build --release
```

### REPL で起動

```bash
cargo run -- repl
# または引数なし
cargo run
```

```
Aegis OMS REPL (type 'help' for commands, 'quit' to exit)
aegis> submit USD/JPY buy limit 150.000 10000
Order created: 01J...
aegis> list
aegis> account
aegis> quit
```

### ワンショット CLI

```bash
# 新規注文
cargo run -- submit --instrument USD/JPY --side buy --type limit --price 150.000 --qty 10000

# 注文一覧
cargo run -- list

# 約定シミュレーション
cargo run -- fill <ORDER_ID> --qty 10000 --price 150.005

# 口座 / ポジション
cargo run -- account
cargo run -- positions

# 注文キャンセル
cargo run -- cancel <ORDER_ID>
```

---

## 📋 サポート銘柄（デフォルト）

| Symbol  | Asset Class | Tick Size | Lot Size | Leverage |
| :------ | :---------- | --------: | -------: | -------: |
| USD/JPY | FX          |     0.001 |     1000 |      x25 |
| EUR/USD | FX          |   0.00001 |     1000 |      x25 |
| BTC/USD | Crypto      |      0.01 |    0.001 |       x2 |
| ETH/USD | Crypto      |      0.01 |     0.01 |       x2 |

初期口座残高: **100,000**（`acc-001` / "Default"）

---

## 🗄 データベーススキーマ（任意）

`sql/` 配下に PostgreSQL 用の DDL を同梱しています。

```bash
psql -d aegis_oms -f sql/000_drop_all.sql
psql -d aegis_oms -f sql/001_create_tables.sql
psql -d aegis_oms -f sql/002_seed_data.sql
```

> 現状の Rust 実装は in-memory ストアです。永続化はスキーマのみ提供しています。

---

## 🧪 テスト

```bash
cargo test
```

開発用依存として `pretty_assertions` を採用しています。

---

## 🛠 開発

主要 crate:
- [`clap`](https://crates.io/crates/clap) — CLI パーサ
- [`rust_decimal`](https://crates.io/crates/rust_decimal) — 高精度な10進演算
- [`ulid`](https://crates.io/crates/ulid) — Order ID
- [`chrono`](https://crates.io/crates/chrono) — タイムスタンプ
- [`serde`](https://crates.io/crates/serde) — シリアライズ
- [`thiserror`](https://crates.io/crates/thiserror) — エラー定義
- [`tracing`](https://crates.io/crates/tracing) — 構造化ログ

リポジトリには `pre-commit`、`cliff.toml`（CHANGELOG 生成）、
`deny.toml`（cargo-deny）、`_typos.toml` などの開発支援設定も含まれます。

---

## 📜 ライセンス

本プロジェクトは [MIT License](./LICENSE) の下で公開されています。

---

## ⚠️ 免責事項

本ソフトウェアは学習・研究用のリファレンス実装です。
実際の取引・本番環境での利用を想定したものではなく、
これによって生じたいかなる損害についても作者は責任を負いません。

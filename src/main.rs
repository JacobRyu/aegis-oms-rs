mod cli;

use clap::Parser;

fn main() {
    tracing_subscriber::fmt::init();
    let cli = cli::Cli::parse();
    let mut svc =
        if let Some(db_url) = &cli.db { create_pg_service(db_url) } else { cli::create_service() };

    match cli.command {
        Some(cli::Commands::Submit { instrument, side, r#type, price, qty }) => {
            cli::handlers::handle_submit(&mut svc, &instrument, &side, &r#type, price, qty)
        }
        Some(cli::Commands::List) => cli::handlers::handle_list(&svc),
        Some(cli::Commands::Cancel { id }) => cli::handlers::handle_cancel(&mut svc, &id),
        Some(cli::Commands::Fill { id, qty, price }) => {
            cli::handlers::handle_fill(&mut svc, &id, qty, price)
        }
        Some(cli::Commands::Deposit { amount }) => cli::handlers::handle_deposit(&mut svc, amount),
        Some(cli::Commands::Withdraw { amount }) => {
            cli::handlers::handle_withdraw(&mut svc, amount)
        }
        Some(cli::Commands::Account) => cli::handlers::handle_account(&svc),
        Some(cli::Commands::Positions) => cli::handlers::handle_positions(&svc),
        Some(cli::Commands::History { instrument }) => {
            cli::handlers::handle_history(&svc, instrument.as_deref())
        }
        Some(cli::Commands::Margin { prices }) => {
            let mark_prices = cli::handlers::parse_mark_prices(&prices);
            cli::handlers::handle_margin(&svc, &mark_prices);
        }
        Some(cli::Commands::Repl) | None => cli::repl::run_repl(&mut svc),
    }
}

fn create_pg_service(db_url: &str) -> aegis_oms::service::order_service::OrderService {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let pool = rt.block_on(async {
        aegis_oms::infra::db::create_pool(db_url).await.expect("Failed to connect to PostgreSQL")
    });

    rt.block_on(async {
        aegis_oms::infra::db::run_migrations(&pool)
            .await
            .expect("Failed to run database migrations");
    });

    let store = Box::new(aegis_oms::infra::pg_order_repo::PgOrderRepository::new(pool.clone()));
    let trade_store =
        Box::new(aegis_oms::infra::pg_trade_repo::PgTradeRepository::new(pool.clone()));
    let mut account_repo =
        aegis_oms::infra::pg_account_repo::PgAccountRepository::new(pool.clone());

    let cfg = aegis_oms::config::AppConfig::load();
    let account =
        aegis_oms::domain::account::Account::new("acc-001", "Default", cfg.account.initial_balance);
    let instruments = cli::default_instruments();
    let risk = aegis_oms::service::risk_check::RiskChecker::new(
        aegis_oms::service::risk_check::RiskLimits::default(),
    );
    let bus = aegis_oms::infra::event_bus::EventBus::new();

    let mut svc = aegis_oms::service::order_service::OrderService::with_repos(
        account,
        instruments,
        risk,
        bus,
        store,
        trade_store,
    );

    // P3-4: restore session state from database
    let positions: Vec<(String, aegis_oms::domain::order::Side, rust_decimal::Decimal, rust_decimal::Decimal)> = rt.block_on(async {
        #[derive(sqlx::FromRow)]
        struct PosRow {
            instrument: String,
            side: String,
            quantity: rust_decimal::Decimal,
            avg_price: rust_decimal::Decimal,
        }
        let rows: Vec<PosRow> = sqlx::query_as(
            "SELECT instrument, side, quantity, avg_price FROM positions WHERE account_id = 'acc-001' AND quantity > 0"
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        rows.into_iter().filter_map(|r| {
            let side = match r.side.as_str() {
                "buy" => Some(aegis_oms::domain::order::Side::Buy),
                "sell" => Some(aegis_oms::domain::order::Side::Sell),
                _ => None,
            }?;
            Some((r.instrument, side, r.quantity, r.avg_price))
        }).collect()
    });

    if let Err(e) = svc.restore_from_db(&mut account_repo, "acc-001", positions) {
        tracing::warn!(error = %e, "Failed to restore session state from database");
    }

    tracing::info!("PostgreSQL persistence enabled");
    svc
}

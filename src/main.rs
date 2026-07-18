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

    let account = aegis_oms::domain::account::Account::new(
        "acc-001",
        "Default",
        rust_decimal_macros::dec!(100000),
    );
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

    // P3-4: セッション間状態の復元
    let account_balance = rt.block_on(async {
        sqlx::query_scalar::<_, rust_decimal::Decimal>(
            "SELECT balance FROM accounts WHERE id = 'acc-001'",
        )
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
    });

    if let Some(balance) = account_balance {
        let acc = svc.get_account_mut();
        acc.balance = balance;
        tracing::info!(%balance, "Restored account balance from database");
    }

    tracing::info!("PostgreSQL persistence enabled");
    svc
}

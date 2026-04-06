use sqlx::PgPool;
use tracing::info;

pub async fn connect(database_url: &str) -> PgPool {
    info!("Connecting to database...");
    PgPool::connect(database_url)
        .await
        .expect("Failed to connect to Postgres")
}

pub async fn migrate(pool: &PgPool) {
    info!("Running database migrations...");
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .expect("Failed to run database migrations");
}

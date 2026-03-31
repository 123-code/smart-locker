use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn init_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

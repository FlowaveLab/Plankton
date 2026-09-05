use sqlx::{
    migrate::{MigrateError, Migration, Migrator},
    SqlitePool,
};

pub(crate) async fn run(pool: &SqlitePool) -> Result<(), MigrateError> {
    match sqlx::migrate!("./migrations").run(pool).await {
        Err(MigrateError::VersionMismatch(1)) => published_whitespace_variant().run(pool).await,
        result => result,
    }
}

// 0.2.0 and 0.2.1 accidentally removed one final LF from migration 1.
// Accept only that exact equivalent migration, retaining SQLx validation of all
// checksums and leaving the database's migration records and user data intact.
fn published_whitespace_variant() -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    let migration = &mut migrator.migrations.to_mut()[0];
    assert_eq!(migration.version, 1);
    let sql = migration
        .sql
        .strip_suffix('\n')
        .expect("canonical migration 1 ends with LF")
        .to_owned();
    *migration = Migration::new(
        migration.version,
        migration.description.clone(),
        migration.migration_type,
        sql.into(),
        migration.no_tx,
    );
    migrator
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    async fn assert_preserved(migrator: Migrator) {
        let pool = pool().await;
        migrator.run(&pool).await.unwrap();
        let checksum: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("CREATE TABLE preservation_probe (value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO preservation_probe VALUES ('preserved')")
            .execute(&pool)
            .await
            .unwrap();
        run(&pool).await.unwrap();
        let after: Vec<u8> =
            sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(checksum, after);
        let value: String = sqlx::query_scalar("SELECT value FROM preservation_probe")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, "preserved");
    }

    #[tokio::test]
    async fn original_database_is_preserved() {
        assert_preserved(sqlx::migrate!("./migrations")).await;
    }

    #[tokio::test]
    async fn published_database_is_preserved() {
        assert_preserved(published_whitespace_variant()).await;
    }

    #[tokio::test]
    async fn unknown_checksum_is_rejected() {
        let pool = pool().await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            run(&pool).await,
            Err(MigrateError::VersionMismatch(1))
        ));
    }

    #[tokio::test]
    async fn other_migration_mismatch_is_rejected_after_fallback() {
        let pool = pool().await;
        published_whitespace_variant().run(&pool).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 2")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            run(&pool).await,
            Err(MigrateError::VersionMismatch(2))
        ));
    }

    #[test]
    fn accepted_checksums_are_pinned() {
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }
        assert_eq!(
            hex(&sqlx::migrate!("./migrations").migrations[0].checksum),
            "fc4d1af26dcb7423f78c238e420228c87bdf4542afaab95b79e8cd6901edbc1134999c0b3bf033ebd8ef7337128c0df3"
        );
        assert_eq!(
            hex(&published_whitespace_variant().migrations[0].checksum),
            "5aed6d5df4ba3ad146f807a21c6299ce44c35ad36c0742594cfaba2ff9404a94e626467330d6dcc5d54ea5db428bc7d0"
        );
    }
}

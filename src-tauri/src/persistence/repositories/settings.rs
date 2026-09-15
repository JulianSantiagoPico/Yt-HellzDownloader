use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::{
    domain::{entities::AppSettings, states::OrganizationMode},
    filesystem::ExistingFilePolicy,
};

pub async fn get_settings(pool: &SqlitePool) -> Result<AppSettings, sqlx::Error> {
    let rows = sqlx::query("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await?;

    let mut settings = AppSettings::default();

    for row in rows {
        let key: String = row.get("key");
        let val: String = row.get("value");

        match key.as_str() {
            "default_output_directory" => settings.default_output_directory = val,
            "organization_mode" => {
                if let Ok(mode) = OrganizationMode::from_str(&val) {
                    settings.organization_mode = mode;
                }
            }
            "existing_file_policy" => {
                settings.existing_file_policy = match val.as_str() {
                    "reuse" => ExistingFilePolicy::Reuse,
                    "overwrite" => ExistingFilePolicy::Overwrite,
                    "rename" => ExistingFilePolicy::Rename,
                    _ => ExistingFilePolicy::Ask,
                };
            }
            "max_concurrent_downloads" => {
                if let Ok(v) = val.parse::<u32>() {
                    settings.max_concurrent_downloads = v;
                }
            }
            "max_concurrent_conversions" => {
                if let Ok(v) = val.parse::<u32>() {
                    settings.max_concurrent_conversions = v;
                }
            }
            "max_retries" => {
                if let Ok(v) = val.parse::<u32>() {
                    settings.max_retries = v;
                }
            }
            "check_updates" => {
                settings.check_updates = val != "false";
            }
            _ => {}
        }
    }

    Ok(settings)
}

pub async fn update_settings(
    pool: &SqlitePool,
    settings: &AppSettings,
) -> Result<AppSettings, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let pairs = [
        (
            "default_output_directory",
            settings.default_output_directory.clone(),
        ),
        (
            "organization_mode",
            settings.organization_mode.as_str().to_string(),
        ),
        (
            "existing_file_policy",
            settings.existing_file_policy.as_str().to_string(),
        ),
        (
            "max_concurrent_downloads",
            settings.max_concurrent_downloads.to_string(),
        ),
        (
            "max_concurrent_conversions",
            settings.max_concurrent_conversions.to_string(),
        ),
        ("max_retries", settings.max_retries.to_string()),
        ("check_updates", settings.check_updates.to_string()),
    ];

    for (key, val) in pairs {
        sqlx::query(
            r#"
            INSERT INTO settings (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(key)
        .bind(val)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(settings.clone())
}

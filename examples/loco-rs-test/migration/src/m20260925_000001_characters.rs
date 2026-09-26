use loco_rs::schema::{create_table_without_timestamps, drop_table, ColType};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        create_table_without_timestamps(
            manager,
            "characters",
            &[
                ("id", ColType::PkAuto),
                ("name", ColType::String),
                ("race", ColType::String),
                ("role", ColType::String),
                ("power_level", ColType::BigInteger),
                ("home_planet", ColType::String),
                ("description", ColType::Text),
                ("transformations", ColType::Text),
                ("allies", ColType::Text),
            ],
            &[],
        )
        .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        drop_table(manager, "characters").await
    }
}

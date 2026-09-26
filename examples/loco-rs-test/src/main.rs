use loco_rs::prelude::Result;
use sample_migration::Migrator;

mod app;
mod controllers;
pub mod models;
mod settings;
mod views;

#[tokio::main]
async fn main() -> Result<()> {
    loco_rs::cli::main::<app::App, Migrator>().await
}

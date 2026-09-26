use async_trait::async_trait;
use loco_inertia::{InertiaConfig, InertiaLayer, SsrConfig, ViteDevConfig};
use loco_rs::{
    app::{AppContext, Hooks},
    bgworker::Queue,
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    environment::Environment,
    prelude::{EntityTrait, PaginatorTrait, Result, Set, ViewRenderer},
    task::Tasks,
};
use sample_migration::Migrator;
use std::path::Path;

use crate::{controllers, models, settings};

pub struct App;

#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    async fn connect_workers(_ctx: &AppContext, _queue: &Queue) -> Result<()> {
        Ok(())
    }

    fn register_tasks(_tasks: &mut Tasks) {}

    async fn truncate(ctx: &AppContext) -> Result<()> {
        loco_rs::db::truncate_table(&ctx.db, models::characters::Entity).await?;
        Ok(())
    }

    async fn seed(ctx: &AppContext, _path: &Path) -> Result<()> {
        if models::characters::Entity::find().count(&ctx.db).await? > 0 {
            return Ok(());
        }

        let characters = [
            (
                "Goku",
                "Saiyan",
                "Z Fighter",
                9_000_000,
                "Earth",
                "A cheerful martial artist who protects Earth and seeks stronger opponents.",
                r#"["Super Saiyan","Super Saiyan 2","Super Saiyan 3","Super Saiyan God","Super Saiyan Blue"]"#,
                r#"["Vegeta","Gohan","Piccolo"]"#,
            ),
            (
                "Vegeta",
                "Saiyan",
                "Prince of Saiyans",
                8_500_000,
                "Planet Vegeta",
                "Proud prince whose rivalry with Goku drives him to surpass his limits.",
                r#"["Super Saiyan","Super Saiyan 2","Majin Vegeta","Super Saiyan Blue"]"#,
                r#"["Goku","Bulma","Trunks"]"#,
            ),
            (
                "Gohan",
                "Half-Saiyan",
                "Scholar / Z Fighter",
                7_200_000,
                "Earth",
                "Goku's son: a gentle scholar with extraordinary potential.",
                r#"["Great Saiyaman","Super Saiyan 2","Ultimate Gohan"]"#,
                r#"["Piccolo","Goku","Videl"]"#,
            ),
            (
                "Piccolo",
                "Namekian",
                "Z Fighter",
                5_100_000,
                "Namek",
                "A strategic warrior and mentor who became one of Gohan's closest family.",
                r#"["Fused with Nail","Fused with Kami"]"#,
                r#"["Gohan","Dende","Goku"]"#,
            ),
            (
                "Bulma",
                "Human",
                "Inventor",
                0,
                "Earth",
                "Brilliant inventor whose technology repeatedly saves the world.",
                "[]",
                r#"["Vegeta","Goku","Trunks"]"#,
            ),
            (
                "Frieza",
                "Frost Demon",
                "Galactic Emperor",
                12_000_000,
                "Unknown",
                "Ruthless emperor responsible for the destruction of Planet Vegeta.",
                r#"["First Form","Final Form","Golden Frieza"]"#,
                r#"["King Cold","Goku"]"#,
            ),
        ];

        let rows = characters.into_iter().map(
            |(name, race, role, power_level, home_planet, description, transformations, allies)| {
                models::characters::ActiveModel {
                    name: Set(name.to_owned()),
                    race: Set(race.to_owned()),
                    role: Set(role.to_owned()),
                    power_level: Set(power_level),
                    home_planet: Set(home_planet.to_owned()),
                    description: Set(description.to_owned()),
                    transformations: Set(transformations.to_owned()),
                    allies: Set(allies.to_owned()),
                    ..Default::default()
                }
            },
        );
        models::characters::Entity::insert_many(rows)
            .exec(&ctx.db)
            .await?;
        Ok(())
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::empty()
            .add_route(controllers::characters::routes())
            .add_route(controllers::episodes::routes())
    }

    async fn after_routes(router: axum::Router, ctx: &AppContext) -> Result<axum::Router> {
        let root_view = loco_rs::controller::views::engines::TeraView::build()?;
        let settings = settings::AppSettings::from_context(ctx)?;
        let mut config = InertiaConfig::default();
        let mut vite = ViteDevConfig::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/hot"),
            "app.jsx",
        )
        .react_refresh();
        if let Some(url) = settings.inertia_vite_dev_server {
            vite = vite.server_url(url);
        }
        config.vite_dev_server = Some(vite);
        config.ssr = Some(SsrConfig {
            url: settings.inertia_ssr_url,
            ..SsrConfig::default()
        });
        let config = config.root_view(move |data| {
            root_view
                .render("inertia/root.html", data.clone())
                .map_err(|err| err.to_string())
        });
        Ok(InertiaLayer::new(config).layer(router))
    }
}

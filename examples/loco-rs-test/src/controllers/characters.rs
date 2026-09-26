use loco_inertia::Inertia;
use loco_rs::prelude::*;

use crate::{
    models,
    views::characters::{CharacterDetails, CharacterSummary},
};

pub fn routes() -> Routes {
    Routes::new().get("/", index).get("/characters/{id}", show)
}

#[debug_handler]
async fn index(State(ctx): State<AppContext>) -> Result<Response> {
    let characters = models::characters::Model::all(&ctx.db)
        .await?
        .iter()
        .map(CharacterSummary::from)
        .collect::<Vec<_>>();

    let response = Inertia::render("Characters/Index")
        .props(serde_json::json!({ "characters": characters }))
        .map_err(|err| Error::string(&err.to_string()))?;
    Ok(response.into_response())
}

#[debug_handler]
async fn show(Path(id): Path<i64>, State(ctx): State<AppContext>) -> Result<Response> {
    let character = models::characters::Model::find_by_id(&ctx.db, id).await?;

    let response = Inertia::render("Characters/Show")
        .props(serde_json::json!({
            "character": CharacterDetails::from(&character)
        }))
        .map_err(|err| Error::string(&err.to_string()))?;
    Ok(response.into_response())
}

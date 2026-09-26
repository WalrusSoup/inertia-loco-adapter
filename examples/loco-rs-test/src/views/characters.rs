use serde::Serialize;

use crate::models::characters::Model;

#[derive(Serialize)]
pub struct CharacterSummary {
    id: i64,
    name: String,
    race: String,
    role: String,
    power_level: i64,
    home_planet: String,
}

impl From<&Model> for CharacterSummary {
    fn from(character: &Model) -> Self {
        Self {
            id: character.id,
            name: character.name.clone(),
            race: character.race.clone(),
            role: character.role.clone(),
            power_level: character.power_level,
            home_planet: character.home_planet.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct CharacterDetails {
    character: CharacterSummary,
    description: String,
    transformations: Vec<String>,
    allies: Vec<String>,
}

impl From<&Model> for CharacterDetails {
    fn from(character: &Model) -> Self {
        Self {
            character: CharacterSummary::from(character),
            description: character.description.clone(),
            transformations: serde_json::from_str(&character.transformations).unwrap_or_default(),
            allies: serde_json::from_str(&character.allies).unwrap_or_default(),
        }
    }
}

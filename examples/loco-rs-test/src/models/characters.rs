use loco_rs::prelude::*;

pub use super::_entities::characters::{self, ActiveModel, Entity, Model};

impl Model {
    /// Return all characters ordered by their primary key.
    ///
    /// # Errors
    ///
    /// Returns a model error if the database query fails.
    pub async fn all(db: &DatabaseConnection) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .order_by_asc(characters::Column::Id)
            .all(db)
            .await?)
    }

    /// Find a character by its primary key.
    ///
    /// # Errors
    ///
    /// Returns `ModelError::EntityNotFound` if there is no matching character,
    /// or a model error if the database query fails.
    pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }
}

impl ActiveModelBehavior for ActiveModel {}

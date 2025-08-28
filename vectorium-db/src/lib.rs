pub mod model;
pub mod repository;

use anyhow::Result;
use log::{info, warn};
use qdrant_client::Qdrant;
use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant::{
    CreateCollection, Distance, VectorParams, VectorsConfig, vectors_config::Config,
};

const COLLECTION_NAME: &str = "documents";

pub async fn initialize_db(qdrant_url: &str) -> Result<Qdrant> {
    info!("Connecting to Qdrant at {qdrant_url}");
    let config = QdrantConfig::from_url(qdrant_url);
    let client = Qdrant::new(config)?;
    let collections_list = client.list_collections().await?;
    if !collections_list
        .collections
        .iter()
        .any(|c| c.name == COLLECTION_NAME)
    {
        info!("Creating collection '{COLLECTION_NAME}'.");
        client
            .create_collection(CreateCollection {
                collection_name: COLLECTION_NAME.to_string(),
                vectors_config: Some(VectorsConfig {
                    config: Some(Config::Params(VectorParams {
                        size: 384,
                        distance: Distance::Cosine as i32,
                        ..Default::default()
                    })),
                }),
                ..Default::default()
            })
            .await?;
        info!("Collection '{COLLECTION_NAME}' created successfully.");
    } else {
        warn!("Collection '{COLLECTION_NAME}' already exists.");
    }
    Ok(client)
}

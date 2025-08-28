use anyhow::Result;
use log::{error, info};
use std::env;
use vectorium_db::{initialize_db, repository::DocumentRepository};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let qdrant_url = env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());
    let documents_dir = env::var("DOCUMENTS_DIR").unwrap_or_else(|_| "./data".to_string());

    info!("Starting Vectorium setup...");

    match initialize_db(&qdrant_url).await {
        Ok(qdrant_client) => {
            info!("DB initialized successfully.");
            match DocumentRepository::new(qdrant_client) {
                Ok(mut repository) => {
                    info!("Starting to process documents in {documents_dir}");
                    if let Err(e) = repository
                        .upsert_documents_from_directory(&documents_dir)
                        .await
                    {
                        error!("Failed to process documents: {e}");
                    }
                    info!("Document processing finished.");
                }
                Err(e) => error!("Failed to create DocumentRepository: {e}"),
            }
        }
        Err(e) => error!("Failed to initialize database: {e}"),
    }

    info!("Vectorium setup finished.");
    Ok(())
}

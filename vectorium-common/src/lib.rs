use qdrant_client::Qdrant;
use rust_bert::pipelines::sentence_embeddings::{
    SentenceEmbeddingsBuilder, SentenceEmbeddingsModelType,
};

pub async fn get_embedding(texts: Vec<String>) -> Vec<Vec<f32>> {
    let embeddings = tokio::task::spawn_blocking(move || {
        let sentence_embeddings_model =
            SentenceEmbeddingsBuilder::remote(SentenceEmbeddingsModelType::AllMiniLmL12V2)
                .create_model()
                .expect("Failed to create embeddings model");

        sentence_embeddings_model
            .encode(&texts)
            .expect("Failed to encode sentences")
    })
    .await
    .expect("Failed to join blocking task");

    embeddings
}

pub fn get_qdrant_client() -> Qdrant {
    let client = Qdrant::from_url("http://localhost:6334")
        .build()
        .expect("Failed to build client");
    client
}

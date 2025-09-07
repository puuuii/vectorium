use glob::glob;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, UpsertPointsBuilder, VectorParamsBuilder,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use vectorium_common::get_embedding;
use vectorium_common::get_qdrant_client;

#[tokio::main]
async fn main() {
    let client = get_qdrant_client();

    let collection_name = "knowledge";
    let _ = client.delete_collection(collection_name).await;
    client
        .create_collection(
            CreateCollectionBuilder::new(collection_name)
                .vectors_config(VectorParamsBuilder::new(512, Distance::Cosine)),
        )
        .await
        .expect("Failed to create collection");

    let mut sentences = Vec::new();
    for pattern in &["data/*.txt", "data/*.md"] {
        for entry in glob(pattern).expect("Failed to read glob pattern") {
            if let Ok(path) = entry {
                let file = File::open(path).expect("Failed to open data file");
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    sentences.push(line.expect("Failed to read line"));
                }
            }
        }
    }

    let sentences_clone = sentences.clone();
    let embeddings = get_embedding(sentences_clone).await;

    let points = embeddings
        .into_iter()
        .zip(sentences.iter())
        .enumerate()
        .map(|(i, (embedding, sentence))| {
            let mut payload = HashMap::new();
            payload.insert("text".to_string(), sentence.clone().into());

            PointStruct {
                id: Some((i as u64).into()),
                vectors: Some(embedding.into()),
                payload: payload,
            }
        })
        .collect::<Vec<_>>();

    let _ = client
        .upsert_points(UpsertPointsBuilder::new(collection_name, points).wait(true))
        .await
        .expect("Failed to upsert points");
}

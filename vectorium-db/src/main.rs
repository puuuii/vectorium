use glob::glob;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, UpsertPointsBuilder, VectorParamsBuilder,
};
use rust_bert::pipelines::sentence_embeddings::{
    SentenceEmbeddingsBuilder, SentenceEmbeddingsModelType,
};
use std::fs::File;
use std::io::{BufRead, BufReader};

#[tokio::main]
async fn main() {
    let client = Qdrant::from_url("http://localhost:6334")
        .build()
        .expect("Failed to build client");

    let collection_name = "test_collection";
    let _ = client.delete_collection(collection_name).await;
    client
        .create_collection(
            CreateCollectionBuilder::new(collection_name)
                .vectors_config(VectorParamsBuilder::new(384, Distance::Cosine)),
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
    let embeddings = tokio::task::spawn_blocking(move || {
        let sentence_embeddings_model =
            SentenceEmbeddingsBuilder::remote(SentenceEmbeddingsModelType::AllMiniLmL12V2)
                .create_model()
                .expect("Failed to create embeddings model");

        sentence_embeddings_model
            .encode(&sentences_clone)
            .expect("Failed to encode sentences")
    })
    .await
    .expect("Blocking task failed");

    println!("\n=== 文埋め込み結果 ===");
    for (i, embedding) in embeddings.iter().enumerate() {
        println!("テキスト {}: 次元数 = {}", i + 1, embedding.len());
    }

    let points = embeddings
        .into_iter()
        .zip(sentences.iter())
        .enumerate()
        .map(|(i, (embedding, sentence))| {
            let payload: std::collections::HashMap<String, qdrant_client::qdrant::Value> =
                [("text".into(), sentence.clone().into())]
                    .into_iter()
                    .collect();

            PointStruct {
                id: Some((i as u64).into()),
                vectors: Some(embedding.into()),
                payload,
            }
        })
        .collect::<Vec<_>>();

    let _ = client
        .upsert_points(UpsertPointsBuilder::new(collection_name, points).wait(true))
        .await
        .expect("Failed to upsert points");
}

use axum::{Router, extract::Path, response::Json, routing::get};
use qdrant_client::qdrant::QueryPointsBuilder;
use serde_json::json;
use vectorium_common::get_embedding;
use vectorium_common::get_qdrant_client;

#[tokio::main]
async fn main() {
    let client = get_qdrant_client();

    let app = Router::new().route(
        "/{key}",
        get(|Path(key): Path<String>| async move {
            let query_key = key.clone();
            let embeddings = get_embedding(vec![key]).await;

            let search_result = client
                .query(
                    QueryPointsBuilder::new("knowledge")
                        .query(embeddings[0].clone())
                        .with_payload(true),
                )
                .await
                .expect("Failed to query points");

            dbg!(&search_result);

            Json(json!({
                "key": query_key,
                "results": search_result.result.len(),
                "search_result": format!("{:?}", search_result.result)
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("🚀 Server running on http://localhost:3000");

    axum::serve(listener, app).await.unwrap();
}

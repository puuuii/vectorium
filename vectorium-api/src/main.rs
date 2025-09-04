use axum::{Router, extract::Path, http::StatusCode, response::Json, routing::get};
use qdrant_client::qdrant::QueryPointsBuilder;
use serde_json::json;
use vectorium_common::get_embedding;
use vectorium_common::get_qdrant_client;

#[tokio::main]
async fn main() {
    let client = get_qdrant_client();

    let app = Router::new()
        .route("/favicon.ico", get(|| async { StatusCode::NOT_FOUND }))
        .route(
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

                let values = search_result
                    .result
                    .iter()
                    .map(|point| {
                        point.payload.get("text").and_then(|v| {
                            if let Some(kind) = &v.kind {
                                match kind {
                                    qdrant_client::qdrant::value::Kind::StringValue(s) => {
                                        Some(s.clone())
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        })
                    })
                    .collect::<Vec<Option<String>>>();

                dbg!(&search_result);
                Json(json!({
                    "key": query_key,
                    "results": search_result.result.len(),
                    "search_results": values
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("🚀 Server running on http://localhost:3000");

    axum::serve(listener, app).await.unwrap();
}

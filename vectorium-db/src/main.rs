use qdrant_client::{
    Qdrant,
    qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder},
};

#[tokio::main]
async fn main() {
    let client = Qdrant::from_url("http://localhost:6334")
        .build()
        .expect("Failed to build client");

    client
        .create_collection(
            CreateCollectionBuilder::new("test_collection")
                .vectors_config(VectorParamsBuilder::new(4, Distance::Cosine)),
        )
        .await
        .expect("Failed to create collection");
}

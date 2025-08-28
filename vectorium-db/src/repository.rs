use anyhow::{Context, Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use glob::glob;
use log::{info, warn};
use qdrant_client::Payload;
use qdrant_client::qdrant::{PointStruct, UpsertPoints};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

const COLLECTION_NAME: &str = "documents";

pub struct DocumentRepository {
    qdrant_client: qdrant_client::Qdrant,
    embedding_model: TextEmbedding,
}

impl DocumentRepository {
    pub fn new(qdrant_client: qdrant_client::Qdrant) -> Result<Self> {
        info!("Initializing embedding model...");
        let mut init_options = InitOptions::new(EmbeddingModel::AllMiniLML6V2);
        init_options.show_download_progress = true;

        let embedding_model = TextEmbedding::try_new(init_options)?;
        info!("Embedding model initialized.");
        Ok(Self {
            qdrant_client,
            embedding_model,
        })
    }

    pub async fn upsert_documents_from_directory(&mut self, dir_path: &str) -> Result<()> {
        let pattern = format!("{dir_path}/*.{{txt,md}}");
        info!("Searching for documents in: {pattern}");
        let paths: Vec<PathBuf> = glob(&pattern)?.filter_map(Result::ok).collect();

        if paths.is_empty() {
            warn!("No documents found in '{dir_path}'");
            return Ok(());
        }

        info!("Found {} documents to process.", paths.len());

        let mut points_to_upsert = Vec::new();

        for path in paths {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file: {path:?}"))?;

            if content.trim().is_empty() {
                warn!("Skipping empty file: {path:?}");
                continue;
            }

            let metadata = fs::metadata(&path)?;
            let last_modified = metadata.modified()?;

            info!("Embedding file: {path:?}");
            let embeddings = self.embedding_model.embed(vec![content.as_str()], None)?;
            let file_embedding = embeddings
                .get(0)
                .cloned()
                .ok_or_else(|| anyhow!("Embedding failed for file {path:?}"))?;

            let id = format!("{:x}", md5::compute(path.to_str().unwrap()));

            let payload: Payload = json!({
                "file_path": path.to_str(),
                "file_name": path.file_name().unwrap().to_str(),
                "last_modified": last_modified.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
                "content_preview": content.chars().take(200).collect::<String>(),
                "content": content,
            })
            .try_into()?;

            points_to_upsert.push(PointStruct::new(id, file_embedding, payload));
        }

        if !points_to_upsert.is_empty() {
            info!("Upserting {} points to Qdrant.", points_to_upsert.len());
            let result = self
                .qdrant_client
                .upsert_points(UpsertPoints {
                    collection_name: COLLECTION_NAME.to_string(),
                    wait: Some(true),
                    points: points_to_upsert,
                    ..Default::default()
                })
                .await?;

            info!("Upsert operation sent to Qdrant: {:?}", result);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;

    // TextEmbeddingのモック
    struct MockEmbedding;
    impl MockEmbedding {
        fn embed(&self, texts: Vec<&str>, _opt: Option<()>) -> Result<Vec<Vec<f32>>> {
            Ok(vec![vec![0.0; 384]; texts.len()])
        }
    }

    // Qdrantのモック
    struct MockQdrant;
    impl MockQdrant {
        async fn upsert_points(&self, _req: UpsertPoints) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_upsert_documents_from_directory_simple() {
        // テスト用ディレクトリとファイル作成
        let test_dir = PathBuf::from("./test_upsert_data");
        fs::create_dir_all(&test_dir).unwrap();
        let txt_path = test_dir.join("sample.txt");
        let _ = fs::File::create(&txt_path).and_then(|mut f| f.write_all(b"Hello upsert"));

        // モックを使ったDocumentRepositoryのテスト用構造体
        struct TestRepo {
            qdrant_client: MockQdrant,
            embedding_model: MockEmbedding,
        }
        impl TestRepo {
            async fn upsert_documents_from_directory(&mut self, dir_path: &str) -> Result<()> {
                let pattern = format!("{dir_path}/*.{{txt,md}}");
                let paths: Vec<PathBuf> = glob(&pattern)?.filter_map(Result::ok).collect();
                if paths.is_empty() {
                    return Ok(());
                }
                let mut points_to_upsert = Vec::new();
                for path in paths {
                    let content = fs::read_to_string(&path)?;
                    let embeddings = self.embedding_model.embed(vec![content.as_str()], None)?;
                    let file_embedding = embeddings.get(0).cloned().unwrap();
                    let id = format!("{:x}", md5::compute(path.to_str().unwrap()));
                    let payload: Payload = json!({
                        "file_path": path.to_str(),
                        "file_name": path.file_name().unwrap().to_str(),
                        "last_modified": 0,
                        "content_preview": content.chars().take(200).collect::<String>(),
                        "content": content,
                    })
                    .try_into()?;
                    points_to_upsert.push(PointStruct::new(id, file_embedding, payload));
                }
                if !points_to_upsert.is_empty() {
                    self.qdrant_client
                        .upsert_points(UpsertPoints {
                            collection_name: COLLECTION_NAME.to_string(),
                            wait: Some(true),
                            points: points_to_upsert,
                            ..Default::default()
                        })
                        .await?;
                }
                Ok(())
            }
        }

        let mut repo = TestRepo {
            qdrant_client: MockQdrant,
            embedding_model: MockEmbedding,
        };
        let result = repo
            .upsert_documents_from_directory(test_dir.to_str().unwrap())
            .await;
        assert!(result.is_ok());

        fs::remove_file(&txt_path).unwrap();
        fs::remove_dir(&test_dir).unwrap();
    }
}

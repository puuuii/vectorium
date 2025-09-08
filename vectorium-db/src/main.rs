use anyhow::{Context, Result};
use glob::glob;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, UpsertPointsBuilder, VectorParamsBuilder,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use vectorium_common::get_embedding;
use vectorium_common::get_qdrant_client;

// 設定構造体でマジックナンバーを排除
#[derive(Debug, Clone)]
struct ProcessingConfig {
    chunk_size: usize,
    batch_size: usize,
    buffer_size: usize,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 3000,
            batch_size: 5,
            buffer_size: 64 * 1024,
        }
    }
}

// ファイル処理の結果
struct ProcessingResult {
    total_points: u64,
    points: Vec<PointStruct>,
}

// チャンク処理（関数型スタイル）
async fn process_chunk(chunk: &[String], start_id: u64, title: &str) -> Result<ProcessingResult> {
    println!("Generating embeddings for {} sentences...", chunk.len());

    let embeddings = get_embedding(chunk.to_vec()).await;

    let points: Vec<PointStruct> = embeddings
        .into_iter()
        .zip(chunk.iter())
        .enumerate()
        .map(|(i, (embedding, sentence))| {
            let point_id = start_id + i as u64 + 1;

            let payload = [
                ("title".to_string(), title.to_string().into()),
                ("text".to_string(), sentence.clone().into()),
            ]
            .into_iter()
            .collect::<HashMap<_, _>>();

            PointStruct::new(
                point_id,
                qdrant_client::qdrant::Vectors::from(embedding),
                payload,
            )
        })
        .collect();

    println!("Generated {} embeddings", points.len());

    Ok(ProcessingResult {
        total_points: start_id + points.len() as u64,
        points,
    })
}

// バッチupsert（エラーハンドリング付き）
async fn upsert_batch(
    client: &qdrant_client::Qdrant,
    collection_name: &str,
    batch_points: &mut Vec<PointStruct>,
) -> Result<()> {
    if batch_points.is_empty() {
        return Ok(());
    }

    println!("Batch upserting {} points to Qdrant...", batch_points.len());

    client
        .upsert_points(UpsertPointsBuilder::new(
            collection_name,
            batch_points.clone(),
        ))
        .await
        .context("Failed to upsert points")?;

    batch_points.clear();
    println!("Batch upsert completed");
    Ok(())
}

// ファイルから非空行を読み取るイテレータ
fn read_non_empty_lines(
    file_path: &std::path::Path,
    buffer_size: usize,
) -> Result<impl Iterator<Item = Result<String>>> {
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;

    let reader = BufReader::with_capacity(buffer_size, file);

    Ok(reader
        .lines()
        .map(|line| line.context("Failed to read line"))
        .filter_map(|line| match line {
            Ok(content) if !content.trim().is_empty() => Some(Ok(content)),
            Ok(_) => None, // 空行をスキップ
            Err(e) => Some(Err(e)),
        }))
}

// ファイル処理の中核ロジック
async fn process_file(
    client: &qdrant_client::Qdrant,
    collection_name: &str,
    file_path: std::path::PathBuf,
    config: &ProcessingConfig,
    mut current_id: u64,
) -> Result<u64> {
    let title = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    println!("Processing file: {}", title);

    let lines = read_non_empty_lines(&file_path, config.buffer_size)?;
    let mut chunk_buffer = Vec::with_capacity(config.chunk_size);
    let mut batch_points = Vec::new();

    for line_result in lines {
        let line = line_result?;
        chunk_buffer.push(line);

        // チャンク処理
        if chunk_buffer.len() >= config.chunk_size {
            let result = process_chunk(&chunk_buffer, current_id, &title).await?;
            current_id = result.total_points;
            batch_points.extend(result.points);

            // バッチ処理
            if batch_points.len() >= config.batch_size * config.chunk_size {
                upsert_batch(client, collection_name, &mut batch_points).await?;
            }

            chunk_buffer.clear();
        }
    }

    // 残りのチャンクを処理
    if !chunk_buffer.is_empty() {
        let result = process_chunk(&chunk_buffer, current_id, &title).await?;
        current_id = result.total_points;
        batch_points.extend(result.points);
    }

    // 残りのバッチを処理
    if !batch_points.is_empty() {
        upsert_batch(client, collection_name, &mut batch_points).await?;
    }

    println!("Completed processing file: {}", title);
    Ok(current_id)
}

// コレクション初期化
async fn initialize_collection(
    client: &qdrant_client::Qdrant,
    collection_name: &str,
) -> Result<()> {
    let _ = client.delete_collection(collection_name).await; // エラー無視（存在しない場合）

    client
        .create_collection(
            CreateCollectionBuilder::new(collection_name)
                .vectors_config(VectorParamsBuilder::new(512, Distance::Cosine)),
        )
        .await
        .context("Failed to create collection")?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = get_qdrant_client();
    let collection_name = "knowledge";
    let config = ProcessingConfig::default();

    // コレクション初期化
    initialize_collection(&client, collection_name).await?;
    println!("Loading data from files...");

    // ファイルパターンからファイルリストを取得
    let file_paths: Result<Vec<_>> = ["data/*.txt", "data/*.md"]
        .iter()
        .flat_map(|pattern| {
            glob(pattern)
                .context("Failed to read glob pattern")
                .into_iter()
                .flatten()
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to collect file paths");

    // 各ファイルを順次処理
    let mut current_id = 0u64;
    for file_path in file_paths? {
        current_id = process_file(&client, collection_name, file_path, &config, current_id).await?;
    }

    println!("Processing completed. Total points: {}", current_id);
    Ok(())
}

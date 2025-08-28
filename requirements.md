# Vectorium 要件定義書

## 1. プロジェクト概要

- MCP を通じて生成 AI から利用可能な、テキストファイル検索システム
- MCP サーバー（Rust 実装）と Qdrant ベクターデータベース（Docker）
- 2 つの独立したプロセスで構成

## 2. 技術スタック

- Rust (Edition 2024)
- MCP: rust-mcp-sdk v0.6.1
- Qdrant: Docker, qdrant-client v1.15.0
- 埋め込み: fastembed v5.1.0

## 3. システム構成

- MCP サーバー（vectorium クレート）
- Qdrant DB システム（vectorium-db クレート）
- API（vectorium-api クレート）
- data/documents/ 配下のテキストファイルを対象

## 4. 機能仕様

- DB 初期化・差分更新（ファイル名＋更新日時）
- ベクトル化（fastembed, 384 次元, cosine）
- MCP API（vectorium_search ツール）
- Qdrant コレクション管理
- エラーハンドリング（JSON-RPC, Tool execution error）

## 5. MCP API 仕様

- 入力: query(string), limit(number, default:5, max:20), threshold(number, default:0.7)
- 出力: results(array: filename, similarity_score, content_preview, file_size, last_modified), total_found, query_embedding_time_ms, search_time_ms

## 6. Qdrant 設定

- Docker, コレクション名: documents, ベクトルサイズ: 384, 距離関数: Cosine

## 7. パフォーマンス目標

- 起動: 30 秒以内, 差分更新: 10 秒以内, 検索: 500ms 以内, ファイル数: 1,000

## 8. 設定管理

- 環境変数: QDRANT_URL, DATA_DIR, DOCUMENTS_DIR, CACHE_DIR
- デフォルト: ./data/documents/, ./data/cache/, http://localhost:6333

## 9. 開発マイルストーン

- Phase 1: 基盤実装
- Phase 2: MCP 統合
- Phase 3: 差分更新・最適化

---

## 詳細設計

### アーキテクチャ

- オニオンアーキテクチャ（依存性逆転、ドメイン中心）
- DDD（ドメイン駆動設計）
- クレート分割:
  - vectorium-db: Qdrant 操作・永続化
  - vectorium-api: MCP API・クエリ処理
  - vectorium-domain: ドメインモデル・ビジネスロジック
  - vectorium-app: アプリケーションサービス

### レイヤ構成

```
┌─────────────┐
│ Presentation│ vectorium-api
├─────────────┤
│ Application │ vectorium-app
├─────────────┤
│ Domain      │ vectorium-domain
├─────────────┤
│ Infrastructure│ vectorium-db
└─────────────┘
```

### DDD 設計

- Entity: Document（ファイル名、内容、更新日時、ベクトル）
- ValueObject: Embedding, SimilarityScore
- Repository: DocumentRepository（Qdrant 連携）
- Service: SearchService（検索ロジック）
- UseCase: SearchDocuments

### クレート分割

- vectorium-db: Qdrant 操作、DocumentRepository 実装
- vectorium-api: MCP ツール定義、API エンドポイント
- vectorium-domain: ドメインモデル、ビジネスロジック
- vectorium-app: アプリケーションサービス、ユースケース

### ディレクトリ例

```
vectorium/
├── vectorium-domain/
├── vectorium-db/
├── vectorium-api/
├── vectorium-app/
└── requirements.md
```

### その他

- Qdrant と API は別プロセス・別クレート
- テストは各クレート単位で実施
- 拡張性・保守性重視

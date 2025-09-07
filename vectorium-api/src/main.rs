// 必要なライブラリをインポート（外部依存関係の読み込み）
use anyhow::Result;
// use axum::Json;
// エラーハンドリング用のライブラリ
use rmcp::{ServiceExt, transport::stdio};  // MCPサーバー用のライブラリと標準入出力通信
use tracing_subscriber::{self, EnvFilter};  // ログ出力機能用のライブラリ
// use vectorium_common::get_embedding;
// use vectorium_common::get_qdrant_client;
// use qdrant_client::Qdrant;
// use qdrant_client::qdrant::QueryPointsBuilder;

/// メインプログラムの開始点
///
/// このプログラムは「数値カウンター」を操作できるMCPサーバーです。
/// MCPサーバーとは、Model Context Protocol という通信規格に従って
/// 外部のAIクライアント（ChatGPT、Claude等）と連携するためのサーバーです。
///
/// このサーバーが提供する機能：
/// - カウンターの値を1増やす（increment）
/// - カウンターの値を1減らす（decrement） 
/// - 現在のカウンター値を取得（get_value）
/// - 挨拶メッセージを返す（say_hello）
/// - 受け取ったメッセージをそのまま返す（echo）
/// - 2つの数値の足し算（sum）
/// 
/// 使用例: npx @modelcontextprotocol/inspector cargo run -p mcp-server-examples --example std_io
#[tokio::main]  // 非同期処理を使用するメイン関数であることを指定
async fn main() -> Result<()> {
    // ステップ1: ログ出力システムの初期化
    // プログラムの実行中に何が起こっているかをコンソールに表示するための設定
    tracing_subscriber::fmt()
        // 環境変数からログレベルを読み取り、デフォルトでDEBUGレベル以上を出力
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        // ログをエラー出力（stderr）に送信（通常の出力とは別チャンネル）
        .with_writer(std::io::stderr)
        // ANSI色コードを無効化（シンプルなテキスト出力）
        .with_ansi(false)
        .init();  // ログシステムを開始

    // サーバー起動開始をログに記録
    tracing::info!("MCPサーバーを起動しています");

    // ステップ2: カウンターサーバーのインスタンス作成と起動
    // Counter::new() でカウンター管理構造体を作成
    // .serve(stdio()) で標準入出力を使った通信でサーバーを開始
    // .inspect_err() でエラーが発生した場合のログ出力処理を設定
    let service = Counter::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("サーバー実行中にエラーが発生しました: {:?}", e);
    })?;

    // ステップ3: サーバーが終了するまで無限に待機
    // この行でプログラムは止まり、クライアントからの要求を待ち続けます
    service.waiting().await?;
    
    // プログラムが正常終了した場合にOkを返す
    Ok(())
}


// MCPサーバー実装のために必要な全てのライブラリ
// rmcp = Rust Model Context Protocol の略
use rmcp::{
    ErrorData as McpError,           // MCPプロトコルでのエラー情報構造体
    RoleServer,                      // サーバーの役割を示すタイプ
    ServerHandler,                   // サーバーの基本動作を定義するトレイト
    handler::server::{
        router::{
            prompt::PromptRouter,    // プロンプト（AI用の質問テンプレート）のルーティング
            tool::ToolRouter         // ツール（実行可能な機能）のルーティング
        },
        wrapper::Parameters,         // リクエストパラメータをラップする構造体
    },
    model::*,                        // MCPプロトコルの全データ構造を一括インポート
    prompt_handler,                  // プロンプト処理用のマクロ
    prompt_router,                   // プロンプトルーター生成用のマクロ
    schemars,                        // JSON Schemaの生成用ライブラリ
    service::RequestContext,         // リクエストのコンテキスト情報
    tool,                            // ツール定義用のマクロ
    tool_handler,                    // ツール処理用のマクロ
    tool_router,                     // ツールルーター生成用のマクロ
};
use serde_json::json;                // JSON操作用のライブラリ


/// プロンプト機能で使用する引数用のデータ構造
/// 
/// プロンプトとは、AI言語モデル向けの質問や指示のテンプレートのことです。
/// この構造体は、プロンプトに埋め込むメッセージを受け取るために使用します。
/// 
/// 各特性の説明:
/// - Debug: デバッグ時に構造体の中身を確認できる
/// - Serialize: Rustの構造体からJSON形式に変換できる  
/// - Deserialize: JSON形式からRustの構造体に変換できる
/// - JsonSchema: 型定義をJSON Schemaとしてクライアントに提供
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ExamplePromptArgs {
    /// プロンプト内に埋め込まれるメッセージ文字列
    /// 例: "今日の天気について教えて" のような文字列
    pub message: String,
}


/// メインのカウンターサーバー構造体
/// 
/// この構造体が、MCPサーバー全体の中核となります。
/// 以下の3つの主要な機能を持ちます：
/// 
/// 1. counter: 実際のカウンター値を保存（複数のスレッドから安全にアクセス可能）
/// 2. tool_router: ツール機能（increment, decrementなど）への要求をルーティング
/// 3. prompt_router: プロンプト機能（テンプレート生成など）への要求をルーティング
/// 
/// Cloneトレイト: この構造体のコピーを作成できるようになります
/// （実際にはArcとMutexのおかげで、同じデータを指すコピーが作られる）
#[derive(Clone)]
pub struct Counter {
    /// ツール機能のルーター
    /// クライアントからの「increment」「decrement」などのツール呼び出し要求を
    /// 適切な処理関数に振り分ける役割を持ちます
    tool_router: ToolRouter<Counter>,
    
    /// プロンプト機能のルーター  
    /// クライアントからのプロンプト生成要求を適切な処理関数に振り分ける役割
    prompt_router: PromptRouter<Counter>,

//    client: Qdrant,
}

// Counter構造体にツール機能を実装するための実装ブロック
// 
// #[tool_router] マクロの意味:
// このマクロにより、以下で定義する各ツール関数が自動的にMCPプロトコルの
// ツールとして認識され、外部クライアントから呼び出し可能になります
#[tool_router]
impl Counter {
    /// Counter構造体の新しいインスタンス（実体）を作成する関数
    ///
    /// #[allow(dead_code)] の意味:
    /// Rustコンパイラの「使われていないコード」警告を無効化します。
    /// この関数はmain関数から呼び出されるので実際は使用されていますが、
    /// 場合によっては警告が出ることがあるため念のため付けています。
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            // ツールルーターを自動生成して設定
            // Self::tool_router() はマクロによって自動生成される関数
            tool_router: Self::tool_router(),

            // プロンプトルーターを自動生成して設定
            // Self::prompt_router() はマクロによって自動生成される関数
            prompt_router: Self::prompt_router(),

            // client: get_qdrant_client(),
        }
    }

    /// リソース作成のヘルパー関数（現在は使用されていない例示用）
    ///
    /// リソース = MCPプロトコルで定義されるデータの単位
    /// （ファイルの内容、メモ、設定情報など）
    ///
    /// 引数:
    /// - uri: リソースの一意識別子（例: "file:///path/to/file.txt"）  
    /// - name: リソースの表示名
    ///
    /// 戻り値: Resource型のオブジェクト
    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        // RawResource::new() でリソースを作成し、注釈なしで返す
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    /// ツール機能5: 受け取ったデータをそのまま返すエコー機能
    ///
    /// Parameters<JsonObject> の意味:
    /// - JsonObject: JSON形式の任意のオブジェクト型
    /// - Parameters(): MCPプロトコルでのパラメータ受け渡し用のラッパー
    /// - object: 実際に受け取ったJSONオブジェクトの中身
    #[tool(description = "受け取った内容をそのまま返します（エコー機能）")]
    fn echo(&self, Parameters(object): Parameters<JsonObject>) -> Result<CallToolResult, McpError> {
        // 受け取ったJSONオブジェクトを文字列形式に変換して返す
        // serde_json::Value::Object(object) でJSON値として扱い
        // .to_string() で文字列に変換
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::Value::Object(object).to_string(),
        )]))
    }

    // #[tool(description = "DBからデータを取得します")]
    // async fn fetch_data(&self, Parameters(object): Parameters<JsonObject>) -> Result<CallToolResult, McpError> {
    //     let query_key = serde_json::Value::Object(object).to_string();
    //     let embeddings = get_embedding(vec![query_key]).await;

    //     let search_result = self.client
    //         .query(
    //             QueryPointsBuilder::new("knowledge")
    //                 .query(embeddings[0].clone())
    //                 .with_payload(true),
    //         )
    //         .await
    //         .expect("Failed to query points");

    //     let values = search_result
    //         .result
    //         .iter()
    //         .filter_map(|point| {
    //             point.payload.get("text").and_then(|v| {
    //                 if let Some(kind) = &v.kind {
    //                     match kind {
    //                         qdrant_client::qdrant::value::Kind::StringValue(s) => {
    //                             Some(s.clone())
    //                         }
    //                         _ => None,
    //                     }
    //                 } else {
    //                     None
    //                 }
    //             })
    //         })
    //         .collect::<Vec<String>>();

    //     let result_text = values.join("\n\n");

    //     Ok(CallToolResult::success(vec![Content::text(result_text)]))
    // }
}

// Counter構造体にプロンプト機能を実装するための実装ブロック
//
// #[prompt_router] マクロの意味:
// このマクロにより、以下で定義するプロンプト生成関数が自動的にMCPプロトコルの
// プロンプト機能として認識され、外部クライアントから呼び出し可能になります
//
// プロンプトとは？
// AI言語モデル（GPT、Claude等）に送る質問や指示のテンプレートのことです。
// クライアントはこのサーバーに「こんな状況でAIに何を聞けば良い？」と問い合わせ、
// サーバーが「この文章をAIに送ってください」という形でプロンプトを返します。
#[prompt_router]
impl Counter {
}

// MCPサーバーとしての基本機能を実装するための実装ブロック
//
// #[tool_handler] マクロ: 
// この構造体がツール機能のハンドリング（処理）を行うことを宣言
//
// #[prompt_handler] マクロ:
// この構造体がプロンプト機能のハンドリングを行うことを宣言
//
// ServerHandler トレイト:
// MCPサーバーとして動作するために必要な基本的な機能を定義するRustのトレイト
// トレイト = 他言語のインターフェースのようなもの
#[tool_handler]
#[prompt_handler]
impl ServerHandler for Counter {
    /// MCPサーバーの基本情報をクライアントに返す関数
    /// 
    /// クライアントがサーバーに接続した際、「このサーバーは何ができるか？」
    /// 「どのバージョンのMCPプロトコルに対応しているか？」などの情報を
    /// この関数で返します。
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            // 対応MCPプロトコルバージョン（2024年11月5日版）
            protocol_version: ProtocolVersion::V_2024_11_05,
            
            // サーバーの機能（capability = 能力）を設定
            capabilities: ServerCapabilities::builder()
                .enable_prompts()   // プロンプト機能を有効化
                .enable_resources() // リソース機能を有効化
                .enable_tools()     // ツール機能を有効化
                .build(),           // 設定を完成させる
            
            // サーバーの実装情報（自動的に環境から取得）
            server_info: Implementation::from_build_env(),
            
            // クライアント向けの使用説明書
            instructions: Some(
                "このサーバーはカウンター操作とプロンプト応答機能を提供します。\n\n利用可能なツール:\n- increment: カウンターを1増やす\n- decrement: カウンターを1減らす\n- get_value: 現在のカウンター値を取得\n- say_hello: 挨拶メッセージを返す\n- echo: 送信されたデータをそのまま返す\n- sum: 2つの数値の合計を計算\n\n利用可能なプロンプト:\n- example_prompt: 例示用のプロンプト生成\n- counter_analysis: カウンター分析用のプロンプト生成".to_string()
            ),
        }
    }

    /// サーバーが提供するリソース一覧を返す関数（現在は例示用の固定データ）
    /// 
    /// リソース = MCPプロトコルで扱われるデータの単位
    /// ファイルの内容、設定情報、メモなど様々な情報をリソースとして提供可能
    /// 
    /// 引数:
    /// - _request: ページネーション用の要求パラメータ（今回は未使用）
    /// - _: リクエストコンテキスト（今回は未使用のため変数名省略）
    /// 
    /// 戻り値: ListResourcesResult
    /// 利用可能なリソースのリストと、次のページへのカーソル情報
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            // 例示用のリソース2つを定義
            resources: vec![
                // リソース1: 作業ディレクトリ情報
                self._create_resource_text("str:////Users/to/some/path/", "cwd"),
                // リソース2: メモ情報
                self._create_resource_text("memo://insights", "memo-name"),
            ],
            // 次のページは無いのでNone
            next_cursor: None,
        })
    }

    /// 指定されたリソースの実際の内容を返す関数
    /// 
    /// クライアントが「このリソースの中身を教えて」と要求した際に、
    /// リソースのURI（識別子）に基づいて適切な内容を返します。
    /// 
    /// 引数:
    /// - ReadResourceRequestParam { uri }: リソースのURI
    /// - _: リクエストコンテキスト（未使用）
    /// 
    /// 戻り値: ReadResourceResult
    /// リソースの実際の内容
    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        // URIの文字列値によって処理を分岐
        match uri.as_str() {
            // 作業ディレクトリリソースが要求された場合
            "str:////Users/to/some/path/" => {
                let cwd = "/Users/to/some/path/";
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(cwd, uri)],
                })
            }
            // メモリソースが要求された場合
            "memo://insights" => {
                let memo = "ビジネスインテリジェンスメモ\n\n分析により5つの重要な洞察が明らかになりました...";
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(memo, uri)],
                })
            }
            // 存在しないリソースが要求された場合はエラーを返す
            _ => Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({
                    "uri": uri
                })),
            )),
        }
    }

    /// 利用可能なリソーステンプレート一覧を返す関数（現在は空リスト）
    ///
    /// リソーステンプレート = 動的にリソースを生成するためのテンプレート
    /// 例：「/user/{user_id}/profile」のような形式で、user_idを指定することで
    /// 動的にユーザープロファイルリソースを生成するようなもの
    /// 
    /// このサーバーでは現在テンプレート機能は提供していないため空リストを返します
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            next_cursor: None,           // 次のページはない
            resource_templates: Vec::new(), // テンプレートは提供しない（空リスト）
        })
    }

    /// サーバー初期化処理
    ///
    /// クライアントがサーバーに最初に接続した際に呼び出される関数です。
    /// サーバーの初期設定や、接続情報のログ記録などを行います。
    /// 
    /// 引数:
    /// - _request: 初期化要求パラメータ（今回は未使用）
    /// - context: リクエストのコンテキスト情報
    /// 
    /// 戻り値: InitializeResult
    /// サーバーの基本情報（get_info()と同じ内容）
    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // HTTP経由でサーバーが初期化された場合の特別処理
        // context.extensions.get() でHTTPリクエストの詳細情報を取得可能
        if let Some(http_request_part) = context.extensions.get::<axum::http::request::Parts>() {
            // HTTPヘッダーとURIの情報を取得
            let initialize_headers = &http_request_part.headers;
            let initialize_uri = &http_request_part.uri;
            
            // ログに記録（デバッグ用）
            // ?initialize_headers: ヘッダー情報をDebugフォーマットで出力
            // %initialize_uri: URIを表示フォーマットで出力
            tracing::info!(?initialize_headers, %initialize_uri, "HTTPサーバーから初期化されました");
        }

        // サーバーの基本情報を返す（get_info()と同じ）
        Ok(self.get_info())
    }
}

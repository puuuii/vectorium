// 必要なライブラリをインポート（外部依存関係の読み込み）
use anyhow::Result;  // エラーハンドリング用のライブラリ
use rmcp::{ServiceExt, transport::stdio};  // MCPサーバー用のライブラリと標準入出力通信
use tracing_subscriber::{self, EnvFilter};  // ログ出力機能用のライブラリ

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

// 標準ライブラリのスレッド安全な共有データ構造体
use std::sync::Arc;

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
    prompt,                          // プロンプト定義用のマクロ
    prompt_handler,                  // プロンプト処理用のマクロ
    prompt_router,                   // プロンプトルーター生成用のマクロ
    schemars,                        // JSON Schemaの生成用ライブラリ
    service::RequestContext,         // リクエストのコンテキスト情報
    tool,                            // ツール定義用のマクロ
    tool_handler,                    // ツール処理用のマクロ
    tool_router,                     // ツールルーター生成用のマクロ
};
use serde_json::json;                // JSON操作用のライブラリ
use tokio::sync::Mutex;              // 非同期処理対応のミューテックス（排他制御）

/// sumツール（足し算機能）で使用するリクエスト用のデータ構造
/// 
/// この構造体は、2つの整数（a, b）を受け取って足し算を行うために使用されます。
/// 
/// 各フィールドの説明:
/// - Debug: デバッグ出力時にこの構造体の内容を表示できるようにする
/// - serde::Deserialize: JSON形式のデータからRustの構造体に変換できるようにする
/// - schemars::JsonSchema: JSON Schemaを自動生成してクライアントに型情報を提供
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StructRequest {
    /// 足し算の1つ目の数値
    pub a: i32,
    /// 足し算の2つ目の数値
    pub b: i32,
}

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

/// カウンター分析プロンプト用の引数データ構造
/// 
/// 現在のカウンター値と目標値を比較して、最適な戦略を提案するための
/// プロンプト機能で使用する引数です。
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CounterAnalysisArgs {
    /// 達成したい目標値（ゴール）
    /// 例: カウンターを100にしたい場合は goal: 100
    pub goal: i32,
    
    /// カウンター操作の戦略（オプション項目）
    /// 'fast': 素早くゴールに到達する戦略
    /// 'careful': 慎重にゴールに到達する戦略
    /// None の場合はデフォルト戦略 'careful' が使用されます
    /// 
    /// #[serde(skip_serializing_if = "Option::is_none")] の意味:
    /// この値がNoneの場合、JSON出力時にこのフィールドを省略する
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
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
    /// カウンター本体の値（i32 = 32ビット整数）
    /// 
    /// Arc<Mutex<i32>> の構造説明:
    /// - i32: 実際のカウンター値（-2,147,483,648 から 2,147,483,647 までの整数）
    /// - Mutex<i32>: 複数のスレッドから同時にアクセスされても安全性を保つ排他制御
    /// - Arc<Mutex<i32>>: 複数のスレッド間でMutexを共有するための参照カウンタ付きポインタ
    /// 
    /// つまり：「複数のスレッドで安全に共有できる、排他制御付きの整数値」
    counter: Arc<Mutex<i32>>,
    
    /// ツール機能のルーター
    /// クライアントからの「increment」「decrement」などのツール呼び出し要求を
    /// 適切な処理関数に振り分ける役割を持ちます
    tool_router: ToolRouter<Counter>,
    
    /// プロンプト機能のルーター  
    /// クライアントからのプロンプト生成要求を適切な処理関数に振り分ける役割
    prompt_router: PromptRouter<Counter>,
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
            // カウンター値を0で初期化
            // Arc::new() で参照カウンタ付きポインタを作成
            // Mutex::new(0) で初期値0の排他制御付き整数を作成
            counter: Arc::new(Mutex::new(0)),
            
            // ツールルーターを自動生成して設定
            // Self::tool_router() はマクロによって自動生成される関数
            tool_router: Self::tool_router(),
            
            // プロンプトルーターを自動生成して設定
            // Self::prompt_router() はマクロによって自動生成される関数
            prompt_router: Self::prompt_router(),
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

    /// ツール機能1: カウンターの値を1増やす
    /// 
    /// #[tool(description = "...")] マクロ:
    /// この関数をMCPツールとして登録し、クライアントに説明を提供します
    /// クライアントはこの説明を見て、何をするツールかを理解できます
    /// 
    /// 戻り値: Result<CallToolResult, McpError>
    /// - 成功時: CallToolResult（実行結果の情報）
    /// - 失敗時: McpError（MCPプロトコルでのエラー情報）
    #[tool(description = "カウンターの値を1増やします")]
    async fn increment(&self) -> Result<CallToolResult, McpError> {
        // ステップ1: カウンターへの排他アクセスを取得
        // .lock().await で他のスレッドがアクセス中の場合は待機
        let mut counter = self.counter.lock().await;
        
        // ステップ2: カウンター値を1増やす
        // *counter でMutexの中身（i32値）にアクセス
        *counter += 1;
        
        // ステップ3: 成功結果をクライアントに返す
        // CallToolResult::success() で成功を表現
        // Content::text() で文字列形式の結果を作成
        // counter.to_string() で数値を文字列に変換
        Ok(CallToolResult::success(vec![Content::text(
            counter.to_string(),
        )]))
    }

    /// ツール機能2: カウンターの値を1減らす
    #[tool(description = "カウンターの値を1減らします")]
    async fn decrement(&self) -> Result<CallToolResult, McpError> {
        // increment()と同様の処理だが、減算を実行
        let mut counter = self.counter.lock().await;
        *counter -= 1;
        Ok(CallToolResult::success(vec![Content::text(
            counter.to_string(),
        )]))
    }

    /// ツール機能3: カウンターの現在の値を取得する
    #[tool(description = "カウンターの現在の値を取得します")]
    async fn get_value(&self) -> Result<CallToolResult, McpError> {
        // カウンター値を読み取り専用でアクセス
        // mutキーワードが無いので値の変更はできません
        let counter = self.counter.lock().await;
        
        // 現在の値をそのまま文字列として返す
        Ok(CallToolResult::success(vec![Content::text(
            counter.to_string(),
        )]))
    }

    /// ツール機能4: 簡単な挨拶を返す
    /// 
    /// この関数はasyncキーワードが付いていません = 非同期処理不要
    /// カウンターにアクセスしないため、即座に結果を返せます
    #[tool(description = "クライアントに挨拶メッセージを返します")]
    fn say_hello(&self) -> Result<CallToolResult, McpError> {
        // 固定の文字列 "hello" を返す
        Ok(CallToolResult::success(vec![Content::text("hello")]))
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

    /// ツール機能6: 2つの数値の足し算を実行
    /// 
    /// Parameters<StructRequest> の意味:
    /// - StructRequest: 上で定義したa, bフィールドを持つ構造体
    /// - Parameters(): パラメータのラッパー  
    /// - StructRequest { a, b }: 構造体のフィールドを分解代入で取り出し
    #[tool(description = "2つの数値の合計を計算します")]
    fn sum(
        &self,
        Parameters(StructRequest { a, b }): Parameters<StructRequest>,
    ) -> Result<CallToolResult, McpError> {
        // a + b を計算して文字列に変換し、結果として返す
        Ok(CallToolResult::success(vec![Content::text(
            (a + b).to_string(),
        )]))
    }
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
    /// プロンプト機能1: 例示用の簡単なプロンプトを生成
    /// 
    /// #[prompt(name = "example_prompt")] の意味:
    /// このプロンプト機能を "example_prompt" という名前で登録します
    /// クライアントは "example_prompt" を指定してこの機能を呼び出せます
    /// 
    /// 引数:
    /// - Parameters(args): ExamplePromptArgs構造体（messageフィールドを含む）
    /// - _ctx: リクエストのコンテキスト情報（今回は使用しないのでアンダースコア付き）
    /// 
    /// 戻り値: Vec<PromptMessage>
    /// AI言語モデルに送信するメッセージのリスト
    #[prompt(name = "example_prompt")]
    async fn example_prompt(
        &self,
        Parameters(args): Parameters<ExamplePromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<Vec<PromptMessage>, McpError> {
        // 受け取ったメッセージを埋め込んだプロンプト文を作成
        let prompt = format!(
            "これは例示用のプロンプトです。あなたからのメッセージはこちらです: '{}'",
            args.message
        );
        
        // PromptMessage として整形して返す
        // role: User = ユーザーからのメッセージとして扱う
        // content: プロンプトの本文
        Ok(vec![PromptMessage {
            role: PromptMessageRole::User,
            content: PromptMessageContent::text(prompt),
        }])
    }

    /// プロンプト機能2: カウンター分析用の高度なプロンプトを生成
    /// 
    /// この機能は現在のカウンター値と目標値を比較して、
    /// AI言語モデルが最適な戦略を提案できるような詳細なプロンプトを生成します
    /// 
    /// 戻り値: GetPromptResult
    /// プロンプトの説明文と、AI言語モデルに送るメッセージのセット
    #[prompt(name = "counter_analysis")]
    async fn counter_analysis(
        &self,
        Parameters(args): Parameters<CounterAnalysisArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        // ステップ1: 戦略が指定されていない場合はデフォルト値を設定
        // unwrap_or_else() = Noneの場合にクロージャー（無名関数）を実行
        let strategy = args.strategy.unwrap_or_else(|| "careful".to_string());
        
        // ステップ2: 現在のカウンター値を取得
        let current_value = *self.counter.lock().await;
        
        // ステップ3: 目標値との差を計算
        let difference = args.goal - current_value;

        // ステップ4: AI言語モデル用のメッセージ系列を作成
        // 最初にアシスタントが分析を行うことを宣言し、
        // 続いてユーザーが具体的な状況情報を提供するという流れ
        let messages = vec![
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                "カウンターの状況を分析して、目標達成のための最適なアプローチを提案します。",
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                format!(
                    "現在のカウンター値: {}\n目標値: {}\n差分: {}\n希望する戦略: {}\n\n状況を分析して、目標達成のための最適なアプローチを提案してください。",
                    current_value, args.goal, difference, strategy
                ),
            ),
        ];

        // ステップ5: 完成したプロンプト結果を返す
        Ok(GetPromptResult {
            // このプロンプトの簡単な説明（クライアント表示用）
            description: Some(format!(
                "現在値{}から目標値{}到達のためのカウンター分析",
                current_value, args.goal
            )),
            // AI言語モデルに送信するメッセージ系列
            messages,
        })
    }
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

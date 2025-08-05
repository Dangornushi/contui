mod todo_manager;
mod config;
mod gemini;
pub mod app;
mod history;
mod file_access;
mod markdown;

use crossterm::{
    event::{self, Event},
};
use ratatui::{
    Terminal,
};
use std::{
    time::Duration,
};
use anyhow::Result;
use app::{ChatApp, ChatEvent};
use config::Config;
use gemini::GeminiClient;
use history::HistoryManager;
use app::terminal_util::{setup_terminal, cleanup_terminal};

#[tokio::main]
async fn main() -> Result<()> {
    // プログラム開始時にデバッグログを初期化
    if let Err(e) = std::fs::File::create("contui_debug.log") {
        eprintln!("Failed to initialize debug log: {}", e);
    }
    
    println!("Starting contui application...");
    
    // 設定を読み込む
    println!("Loading configuration...");
    let config = Config::load()?;
    println!("Configuration loaded successfully");
    
    // 履歴管理を初期化
    println!("Initializing history manager...");
    let history_manager = HistoryManager::new()?;
    println!("History manager initialized");
    
    // Geminiクライアントを作成
    println!("Creating Gemini client...");
    let gemini_client = GeminiClient::new(config.llm);
    println!("Gemini client created");

    // ターミナルをセットアップ
    println!("Setting up terminal...");
    let mut terminal = setup_terminal()?;
    println!("Terminal setup complete");

    
    // アプリケーションを作成
    println!("Creating chat application...");
    let mut app = ChatApp::new(gemini_client, history_manager);
    println!("Chat application created");
    

    let result = run_app(&mut app, &mut terminal).await;

    // ターミナルをクリーンアップ
    cleanup_terminal(&mut terminal)?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

async fn run_app(
    app: &mut ChatApp,
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            app.render(f);
        })?;

        // イベントを非ブロッキングで処理
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key_event(key, terminal).await? {
                        // 「q」などの終了要求時のみabort
                        if let Some(handle) = app.llm_task_handle.take() {
                            handle.abort();
                            app.is_loading = false;
                            app.ui.input_mode = app::InputMode::Normal;
                            // abort時に必ずエラーイベント送信
                            let _ = app.event_sender.send(app::ChatEvent::Error("LLMタスクがabortされました".to_string()));
                        }
                        return Ok(());
                    }
                    // 通常の入力時はabortしない
                }
                Event::Resize(_, _) => {
                    // リサイズイベントを処理
                }
                _ => {}
            }
        }

        // チャットイベントを処理
        while let Ok(chat_event) = app.event_receiver.try_recv() {
            match chat_event {
                ChatEvent::AIResponse(msg) => {
                    if msg.starts_with("[BUFFERED_SEND]") {
                        let buffered = msg.trim_start_matches("[BUFFERED_SEND]").to_string();
                        app.ui.input = buffered;
                        // バッファ送信時は通常のsend_messageを呼ぶ
                        let _ = app.send_message(terminal).await;
                    } else {
                        app.push_ai_progress_message(msg, terminal);
                        // 再描画（次ループで再描画されるが即時反映したい場合はここでも呼ぶ）
                        terminal.draw(|f| app.render(f))?;
                    }
                }
                _ => {
                    app.handle_chat_event(chat_event);
                }
            }
        }
    }
}

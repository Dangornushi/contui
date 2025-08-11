mod config;
mod gemini;
pub mod app;
mod history;
mod file_access;
mod markdown;
mod logger; // Add this line
mod test_function_calling; // Add test module

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
use app::ChatApp;
use config::Config;
use gemini::GeminiClient;
use history::HistoryManager;
use app::terminal_util::{setup_terminal, cleanup_terminal};
use std::sync::{Arc, Mutex};

#[tokio::main]

async fn main() -> Result<()> {
    // プログラム開始時にデバッグログを初期化
    // プログラム開始時にログファイルをリセット
    println!("Resetting log files...");
    if let Err(e) = logger::reset_log_file("contui_debug.log") {
        eprintln!("Failed to reset contui_debug.log: {}", e);
    }
    if let Err(e) = logger::reset_log_file("contui_llm_request.log") {
        eprintln!("Failed to reset contui_llm_request.log: {}", e);
    }
    if let Err(e) = logger::reset_log_file("contui_llm_response.log") {
        eprintln!("Failed to reset contui_llm_response.log: {}", e);
    }

    // プログラム開始時にデバッグログを初期化
    println!("Initializing debug logger...");
    if let Err(e) = logger::init_logger("contui_debug.log") {
        eprintln!("Failed to initialize debug logger: {}", e);
    } else {
        logger::log_debug("Debug logger initialized.");
    }
    
    println!("Starting contui application...");
    
    // 設定を読み込む
    println!("Loading configuration...");
    let config = Config::load()?;
    println!("Configuration loaded successfully");
    
    // 履歴管理を初期化
    println!("Initializing history manager...");
    let history_manager = Arc::new(Mutex::new(HistoryManager::new()?));
    println!("History manager initialized");
    
    // Geminiクライアントを作成
    println!("Creating Gemini client...");
    let gemini_client = GeminiClient::new(config.llm, history_manager.clone());
    println!("Gemini client created");

    // ターミナルをセットアップ
    println!("Setting up terminal...");
    let mut terminal = setup_terminal()?;
    println!("Terminal setup complete");

    
    // アプリケーションを作成
    println!("Creating chat application...");
    let mut app = ChatApp::new(gemini_client, history_manager.clone());
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
            app.handle_chat_event(chat_event);
        }
    }
}

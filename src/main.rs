mod todo_manager;
mod config;
mod gemini;
pub mod app;
mod history;
mod file_access;
mod markdown;
mod test_tui;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    io::stdout,
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
    // LLMループCLIモード
    // LLM_LOOP環境変数の状態を表示
    use crossterm::style::Print;
    use std::io::Write;
    let mut ct_stdout = std::io::stdout();
        println!("LLM自動ループCLIモードを開始します。初回プロンプトを入力してください：");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
    
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
    
    // アプリケーションを作成
    println!("Creating chat application...");
    let mut app = ChatApp::new(gemini_client, history_manager);
    println!("Chat application created");
    

    // ターミナルをセットアップ
    println!("Setting up terminal...");
    let mut terminal = setup_terminal()?;
    println!("Terminal setup complete");

    app.chat_loop_with_progress(input.trim(), &mut terminal).await?;
    let result = run_app(&mut terminal, &mut app).await;

    // ターミナルをクリーンアップ
    cleanup_terminal(&mut terminal)?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut ChatApp,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            app.render(f);
        })?;

        // イベントを非ブロッキングで処理
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key_event(key)? {
                        return Ok(());
                    }
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
                    app.push_ai_progress_message(msg, terminal);
                    // 再描画（次ループで再描画されるが即時反映したい場合はここでも呼ぶ）
                    terminal.draw(|f| app.render(f))?;
                }
                _ => {
                    app.handle_chat_event(chat_event);
                }
            }
        }
    }
}

async fn test_key_input() -> Result<()> {
    println!("Starting key input test...");
    let mut terminal = setup_terminal()?;
    println!("Terminal setup complete for key test");

    loop {
        // 画面を描画
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0)])
                .split(f.area());

            let text = "Key Test - Press 'q' to quit, any other key to test";
            let paragraph = Paragraph::new(text)
                .block(Block::default()
                    .title("Key Input Test")
                    .borders(Borders::ALL));

            f.render_widget(paragraph, chunks[0]);
        })?;

        // イベントを処理
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    println!("Key pressed: {:?}", key);
                    if let KeyCode::Char('q') = key.code {
                        break;
                    }
                }
                other => {
                    println!("Other event: {:?}", other);
                }
            }
        }
    }

    cleanup_terminal(&mut terminal)?;

    println!("Key test completed successfully");
    Ok(())
}

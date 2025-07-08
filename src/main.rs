mod config;
mod gemini;
mod app;
mod history;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::{
    io::stdout,
    time::Duration,
};
use anyhow::Result;
use app::ChatApp;
use config::Config;
use gemini::GeminiClient;
use history::HistoryManager;

#[tokio::main]
async fn main() -> Result<()> {
    // 設定を読み込む
    let config = Config::load("token.toml")?;
    
    // 履歴管理を初期化
    let history_manager = HistoryManager::new()?;
    
    // Geminiクライアントを作成
    let gemini_client = GeminiClient::new(config.llm);
    
    // アプリケーションを作成
    let mut app = ChatApp::new(gemini_client, history_manager);
    
    // ターミナルをセットアップ
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app).await;

    // ターミナルをクリーンアップ
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

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
        terminal.draw(|f| app.render(f))?;

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
            app.handle_chat_event(chat_event);
        }
    }
}

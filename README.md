# ConTUI - Gemini Chat TUI

RatatuiとGemini APIを使用したターミナルベースのチャットアプリケーションです。会話履歴を自動保存し、複数のセッションを管理できます。

## 必要な環境

- Rust 1.70以上
- Gemini API キー

## セットアップ

1. 依存関係をインストール:
```bash
cargo build
```

2. `token.toml`ファイルを作成し、Gemini APIキーを設定:
```toml
[llm]
model = "gemini-2.5-flash"
max_tokens = 2000
gemini_api_key = "YOUR_API_KEY_HERE"
```

## 使用方法

```bash
cargo run
```

### 操作方法

#### Normal Mode（通常モード）
- **'i'**: Insert Mode（挿入モード）に入る
- **'a'**: カーソル位置の次から Insert Mode に入る
- **'q'**: アプリケーションを終了
- **'n'**: 新しいチャットセッションを開始
- **'s'**: 手動で履歴を保存
- **'h'/'j'/'k'/'l'** または **矢印キー**: カーソル移動・スクロール
- **'0'**: 行の先頭に移動
- **'$'**: 行の末尾に移動
- **'x'**: カーソル位置の文字を削除
- **'d'**: 行全体を削除
- **Enter**: メッセージを送信

#### Insert Mode（挿入モード）
- **Esc**: Normal Mode に戻る
- **Enter**: メッセージを送信（空でない場合）または改行
- **Backspace**: 文字を削除
- **矢印キー**: カーソル移動・スクロール
- **文字入力**: 文字を入力
- **@file:path**: ファイルを参照（例：@file:./src/main.rs）

#### File Browser Mode（ファイルブラウザモード）
- **'f'**: Normal Mode からファイルブラウザを開く
- **'j'/'k'** または **矢印キー**: ファイル/ディレクトリ選択
- **Enter**: ファイルを入力フィールドに追加、またはディレクトリに移動
- **Space**: ファイルの選択/選択解除を切り替え
- **'u'**: 親ディレクトリに移動
- **'r'**: ディレクトリ内容を更新
- **'i'**: Insert Mode に切り替え
- **'q'** または **Esc**: Normal Mode に戻る

#### Session List Mode（セッション一覧モード）
- **'S'**: Normal Mode からセッション一覧を開く
- **'j'/'k'** または **矢印キー**: セッション選択
- **Enter**: セッションを切り替え
- **'d'**: セッションを削除
- **'n'**: 新しいセッションを作成
- **'q'** または **Esc**: Normal Mode に戻る

### 画面構成

1. **Chat History**: チャット履歴が表示される
2. **Input**: メッセージ入力エリア（現在のモードを表示）
3. **Help**: 現在のモードに応じた操作説明

## 機能

- **会話履歴の永続化**: 自動的に会話を保存し、再起動時に復元
- **複数セッション管理**: 複数のチャットセッションを管理
- **コンテキスト保持**: 前の会話を参照してより自然な対話
- **ファイル参照**: @file:path 形式でファイル内容をAIに送信
- **ファイル作成**: AIがファイルを作成可能（```create_file:filename 形式）
- **ファイルブラウザ**: 直感的なファイル選択・管理
- **Vi風キーバインディング**: 効率的なテキスト編集
- **リアルタイムチャット**: 非同期でAIとやり取り
- **美しいTUIインターフェース**: 直感的で使いやすいUI
- **Unicode対応**: 日本語を含む多言語対応
- **自動スクロール**: 新しいメッセージに自動でスクロール

## ファイル操作

### ファイル参照
メッセージ内で`@file:path/to/file`を使用してファイル内容をAIに送信できます：
```
@file:./src/main.rs この関数を説明してください
```

### ファイル作成
AIに依頼すると、以下の形式でファイルを作成できます：
```
新しいRustファイルを作成してください
```

AIが応答で以下の形式を使用すると、ファイルが自動的に作成されます：
```
\`\`\`create_file:example.rs
fn main() {
    println!("Hello, world!");
}
\`\`\`
```

### セキュリティ
- ファイルアクセスは設定されたディレクトリ内に制限されます
- 現在のディレクトリとホームディレクトリがデフォルトで許可されます

## 履歴データの保存場所

チャット履歴は以下の場所に保存されます：

- **macOS**: `~/Library/Application Support/contui/chat_history.json`
- **Linux**: `~/.local/share/contui/chat_history.json`
- **Windows**: `%APPDATA%\contui\chat_history.json`

## 技術スタック

- **Ratatui**: TUIライブラリ
- **Crossterm**: クロスプラットフォーム端末操作
- **Tokio**: 非同期ランタイム
- **Reqwest**: HTTP クライアント
- **Serde**: JSON シリアライゼーション
- **TOML**: 設定ファイル形式
- **Chrono**: 日時処理
- **UUID**: セッション識別子生成
- **Dirs**: システムディレクトリの取得

## トラブルシューティング

### APIキーエラー
- `token.toml`ファイルが正しく設定されているか確認
- Gemini APIキーが有効か確認

### ビルドエラー
- Rustのバージョンが1.70以上であることを確認
- `cargo clean && cargo build`を実行

### 表示の問題
- ターミナルのサイズが十分であることを確認
- カラー表示をサポートするターミナルを使用

## ライセンス

MIT License
# contui

# Node Watcher

同一LAN内のSymbol（Catapult）ノードとNEM（NIS1）ノードを監視する、Windows向けデスクトップアプリです。

## 主な機能

- Symbol / NIS1 REST APIによるノード状態・チェーン高・ピア監視
- Symbol WebSocketによるブロック・最終化イベントの即時反映
- 明示したCIDRだけを対象にするLANスキャン
- SQLiteによる履歴保存、時系列グラフ、アラート
- Windows通知、トレイ常駐、自動起動設定
- UTF-8 BOM付きCSVエクスポート

秘密鍵、SSH資格情報、トランザクション送信、ホスト再起動は扱いません。

## 開発環境

- Node.js 22以上
- Rust stable MSVC toolchain
- Visual Studio Build Tools（Desktop development with C++）
- WebView2 Runtime

```powershell
npm install
npm run tauri dev
```

## 品質チェック

```powershell
npm run check
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

## データ

監視DBとログはリポジトリ外のWindowsローカルアプリケーションデータ領域へ保存します。実ノードの内部IPやAPIレスポンスはテストデータへ含めないでください。

## License

Copyright (c) 2026 tikpok. All rights reserved.

このリポジトリは非公開です。明示的な許諾なく利用・複製・再配布できません。


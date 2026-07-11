# Harvest Scope

同一LAN内のSymbol（Catapult）ノードとNEM（NIS1）ノードを監視する、Windows向けデスクトップアプリです。

## 主な機能

- Symbol / NIS1 REST APIによるノード状態・チェーン高・ピア監視
- Symbol WebSocketによるブロック・ファイナライズイベントの即時反映
- 明示したCIDRだけを対象にするLANスキャン
- SQLiteによる履歴保存、時系列グラフ、アラート
- 状態別トレイ常駐、障害詳細へ移動できるWindows通知、自動起動設定
- DNS / TCP / REST / WebSocket / StateHashのワンクリック診断と診断履歴
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

## バージョン管理

アプリの版番号はSemantic Versioning（`MAJOR.MINOR.PATCH`）で管理します。現在の版はサイドバー下部に常時表示されます。

```powershell
# package.json、package-lock.json、Cargo.toml、Cargo.lock、tauri.conf.jsonを一括更新
npm run version:set -- 0.1.5

# 5ファイルの版番号が一致していることを確認
npm run version:check
```

`npm run build`でもビルド前に版番号の一致を自動確認します。

## データ

監視DBとログはリポジトリ外のWindowsローカルアプリケーションデータ領域へ保存します。実ノードの内部IPやAPIレスポンスはテストデータへ含めないでください。

## License

Copyright (c) 2026 tikpok.

このソースコードは公開されていますが、ライセンス条件は `LICENSE` を参照してください。

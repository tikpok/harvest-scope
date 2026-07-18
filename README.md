# Harvest Scope

同一LAN内のSymbol（Catapult）ノードとNEM（NIS1）ノードを監視する、Windows向けデスクトップアプリです。

> [!IMPORTANT]
> このリポジトリはオープンソースではありません。許諾されるのはソースコードの閲覧・学習のみです。利用、複製、改変、配布、サブライセンス、販売、配備は、著作権者の事前の書面による許可がない限り禁止します。正確な条件は [`LICENSE`](LICENSE) を参照してください。

## 主な機能

- Symbol / NIS1 REST APIによるノード状態・チェーン高・ピア監視
- Symbol WebSocketによるブロック・ファイナライズイベントの即時反映
- 明示したCIDRだけを対象にするLANスキャン
- SQLiteによる履歴保存、時系列グラフ、アラート
- 状態別トレイ常駐、障害詳細へ移動できるWindows通知、自動起動設定
- DNS / TCP / REST / WebSocket / StateHashのワンクリック診断と診断履歴
- UTF-8 BOM付きCSVエクスポート

LAN、RFC 1918、localhostのノードは引き続き直接監視できます。HTTPノードはホストとポートを確認してノード単位で許可するまで停止し、HTTPS失敗時にHTTPへ自動切替しません。GETのリダイレクトは同一オリジンで最大3回、POSTのリダイレクトは拒否します。

秘密鍵、SSH資格情報、トランザクション送信、ホスト再起動は扱いません。

## 開発環境

- Node.js 22以上
- Rust 1.89以上（最新stable MSVC toolchain推奨）
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
cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets
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

raw監視データとrawハーベストイベントの既定保持期間は30日です。ハーベストイベントは最大100,000行、ノード単位で毎分12件、DB論理使用量512 MiBを上限とし、上限時は新しいrawイベントだけを破棄します。365日分の集約データは継続して保持します。

## ライセンス

Copyright (c) 2026 tikpok.

このソースコードはsource-availableですが、OSSではありません。許諾は閲覧・学習のみに限定され、利用・複製・改変・配布・配備などは許可されません。GitHub上のフォーク可否は[GitHub利用規約](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service?lang=ja)および[リポジトリのライセンスに関する説明](https://docs.github.com/ja/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository)に従います。独自ライセンスはGitHub機能としてのフォークを技術的に無効化するものではありません。

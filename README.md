# Harvest Scope

Harvest Scopeは、Symbol（Catapult）ノードとNEM（NIS1）ノードの状態をWindows上で確認・監視するデスクトップアプリです。

プログラミング環境を用意しなくても、GitHub ReleasesからWindows用インストーラーをダウンロードしてインストールできます。

> [!IMPORTANT]
> GitHub Releasesで著作権者が公式に配布する未改変のインストーラーは、誰でもダウンロード、インストール、実行できます。ソースコードについて一般に許諾されるのは閲覧・学習のみです。詳しくは「ライセンスと利用条件」を確認してください。

## Windowsへインストールする

### 対応する配布形式

- Windows x64
- ユーザー単位でインストールされるNSISインストーラー
- 開発環境やRust、Node.jsのインストールは不要

### インストール手順

1. [Harvest Scope v1.1.1のReleaseページ](https://github.com/tikpok/harvest-scope/releases/tag/v1.1.1)を開きます。
2. ページ下部の「Assets」から`Harvest Scope_1.1.1_x64-setup.exe`をダウンロードします。
3. ダウンロードしたインストーラーを実行します。
4. インストール完了後、スタートメニューなどから「Harvest Scope」を起動します。

> [!WARNING]
> 現在のインストーラーにはデジタル署名がありません。Windows SmartScreenなどの警告が表示される場合があります。ファイル名と入手元が上記の公式Releaseであることを確認し、組織や端末のセキュリティポリシーに従ってください。

WebView2 Runtimeが端末にない場合、インストーラーがMicrosoftから取得するため、インストール時にインターネット接続が必要になることがあります。

## 最初に行うこと

1. 監視したいSymbolまたはNIS1ノードを登録します。
2. HTTPSノードでは、登録したendpointへ接続して状態を取得します。
3. HTTPノードでは、接続先のホスト名とポートを確認し、ノード単位で利用を許可します。
4. ダッシュボードでノード状態、チェーン高、ピア、アラートを確認します。

LAN内のノード、RFC 1918の内部IP、localhostも直接指定できます。HTTPS接続に失敗しても、HTTPへ自動的に切り替わることはありません。

## 主な機能

- Symbol / NIS1ノードの状態、チェーン高、ピアの監視
- Symbol WebSocketによるブロック・ファイナライズイベントの反映
- 指定したCIDRだけを対象にしたLANスキャン
- 履歴グラフ、アラート、Windows通知
- DNS / TCP / REST / WebSocket / StateHash診断
- UTF-8 BOM付きCSVエクスポート
- Windowsトレイ常駐と自動起動設定

秘密鍵やSSH資格情報の登録、トランザクション送信、ノードホストの再起動は行いません。

## データの保存

監視データとログは、Windowsのローカルアプリケーションデータ領域へ保存されます。

- raw監視データとrawハーベストイベントの既定保持期間：30日
- rawハーベストイベント：最大100,000行
- ノード単位の保存上限：毎分12件
- DB論理使用量の上限：512 MiB
- 集約データの保持期間：365日

上限に達した場合は、新しいrawハーベストイベントだけを破棄し、ノード監視そのものは継続します。

## 開発者向け

ソースから確認する場合は、次の環境を使用します。

- Node.js 22以上
- Rust 1.89以上（最新stable MSVC toolchain推奨）
- Visual Studio Build Tools（Desktop development with C++）
- WebView2 Runtime

```powershell
npm install
npm run tauri dev
```

品質チェックは次のコマンドで実行できます。

```powershell
npm run check
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets
cargo check --manifest-path src-tauri/Cargo.toml --locked --all-targets
```

## ライセンスと利用条件

Copyright (c) 2026 tikpok.

このリポジトリは一般公開されていますが、オープンソースではありません。ソースコードについて一般に許諾されるのは閲覧・学習のみです。

GitHub Releasesで著作権者が公式に配布する未改変のバイナリは、誰でもダウンロード、インストール、実行できます。ソースコードからのビルドや配備、ソースコードまたはバイナリの改変、再配布、再公開、販売、サブライセンスは許可されません。

正確な条件は[`LICENSE`](LICENSE)を確認してください。GitHub上のフォーク可否はGitHubの機能と利用規約に従います。

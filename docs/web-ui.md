# Web UI

```bash
amitoki web --config ./amitoki.toml
```

表示されたURLをブラウザで開く。既定は`127.0.0.1:8710`。React・TypeScript・Tailwind CSSの3画面をViteでビルドし、実行ファイルに同梱している。配布バイナリの利用にNode.js、CDN、外部サーバは不要。

```bash
amitoki web --config ./amitoki.debug.example.toml --pcap ./input.pcap
amitoki web --config ./amitoki.toml --listen 127.0.0.1:8711 \
  --directory ~/.local/share/amitoki/plugins
```

## パケット解析

「PCAPを開く」でclassic PCAP 2.4のEthernetキャプチャを読み込む。PCAPNGは未対応。最大16MiB、表示は先頭1,000件、1回の解析は120秒以内。解析結果が32MiBを超える場合は小さいキャプチャへ分割する。同時解析は1件。

パケットを選ぶと、実行されたStageごとの入力・出力、ヘッダ、解析結果`annotations`、処理時間、送信予定を表示する。異なるフィールドを色付けし、Hexには先頭256バイトを表示する。独自パケット形式のフィールドは、解析Stageが返す`annotations`に表示される。

再生元は`capture`または`<relay>.received`。NICとRelayは起動せず、設定されたStageを再生専用プロセスで実行する。プラグイン自体は実行されるため、信頼したプラグイン・設定を使う。OSサンドボックスは追加していない。

再生には`pipeline`または`pipeline_file`が必要。Web起動時の構成を固定し、設定の編集や本体のreloadが解析中の構成を変えない。再生構成を更新する場合はWebを再起動する。解析失敗時は前のPCAP結果を保持する。Stageの処理失敗はパケットのエラーとして表示する。

同じ詳細をCLIから取得する場合:

```bash
amitoki debug replay --config ./amitoki.toml --pcap ./input.pcap \
  --inspect --limit 1000 --json
```

従来の`--json`には入出力スナップショットを追加しない。`--inspect`を指定したときだけ出力する。

## パイプラインと稼働状況

図は実際の`from`/`to`から構築する。Stageの宣言順から接続順を推測しない。Stageを選ぶとプラグイン名と`on_error`を確認できる。接続文字列やプラグインの設定値はHTTPへ渡さない。

「再生構成」と「稼働中の構成」を切り替えられる。稼働中の構成は本体が最後に適用できた世代を表示し、reloadが失敗しても未適用の設定を表示しない。

稼働状況は同じ設定パス・同じOSユーザの本体へUnixソケットで問い合わせる。収集、送信、注入、フィルタ、拒否、再試行と、各Relayのキュー・破棄・障害を2秒ごとに更新する。接続できない場合は「未取得」。本体停止・未対応の旧版・権限不一致は区別できない。

Relayの障害数は送信・受信の両方を含むため「送受信の一部停止」と表示する。対向ノードやプラグイン内部の接続状態を推測して表示しない。単一`relay`設定では本体の共通カウンタのみ取得できる。イベントは画面を開いてから観測した変化を最大50件保持し、過去の本体ログを取得するものではない。

## ローカルアクセス

Webは本体の転送処理と別プロセスで動く。画面を閉じても転送は継続する。ライブパケットの収集・設定の書き換え・reload操作はWebから行わない。

ループバックアドレスだけで待ち受け、起動URLのフラグメントに含まれる一時トークンをAPI認証に使う。トークンはアクセスログやRefererへ送らず、ブラウザのタブ内に保持する。起動URLを第三者へ渡さない。異なるHost/Originからの要求を拒否し、画面の埋め込みと外部スクリプトをCSPで禁止する。

PCAPは処理中だけ権限0600の一時ファイルへ置く。解析結果はWebプロセスのメモリに保持し、終了時に破棄する。転送本体にPCAPのバイト列やStageごとのスナップショットを保持させない。

## 開発環境

Node.js 24以降を使う。未導入のUbuntu/Debianでは、[Node.js公式配布](https://nodejs.org/en/download)のLinuxバイナリをユーザーのディレクトリへ配置できる。x86_64/ARM64に対応する。

```bash
sudo apt-get update
sudo apt-get install -y curl ca-certificates xz-utils
amitoki_node_version=24.21.0
case "$(uname -m)" in
  x86_64) amitoki_node_arch=x64 ;;
  aarch64) amitoki_node_arch=arm64 ;;
  *) echo '対応CPUはx86_64/ARM64です'; exit 1 ;;
esac
amitoki_node_archive="node-v${amitoki_node_version}-linux-${amitoki_node_arch}.tar.xz"
amitoki_node_directory="$(mktemp -d)"
curl -fsSL "https://nodejs.org/dist/v${amitoki_node_version}/${amitoki_node_archive}" -o "$amitoki_node_directory/$amitoki_node_archive"
curl -fsSL "https://nodejs.org/dist/v${amitoki_node_version}/SHASUMS256.txt" -o "$amitoki_node_directory/SHASUMS256.txt"
(cd "$amitoki_node_directory" && sha256sum --check --ignore-missing SHASUMS256.txt) || exit 1
mkdir -p "$HOME/.local/share/amitoki-node"
tar -xJf "$amitoki_node_directory/$amitoki_node_archive" -C "$HOME/.local/share/amitoki-node" --strip-components=1
export PATH="$HOME/.local/share/amitoki-node/bin:$PATH"
node --version
npm --version
```

新しいターミナルでも使う場合は、この`export PATH=...`をシェルの初期化ファイルへ追加する。ソースは次の順でビルドする。

```bash
bash scripts/build-web.sh
cargo build --release --bin amitoki --locked
```

`scripts/build-web.sh`は`npm ci`、型検査、Viteビルドを行う。`build.rs`が`web/dist`のハッシュ付きアセットを埋め込む。`web/dist`と`node_modules`はGit管理しない。画面を変更した場合は、Rustのビルド前にWebも再ビルドする。CIではWebを一度ビルドし、同じ成果物を両CPUへ埋め込む。Dockerと`setup.sh`、VM試験のビルドにも組み込んでいる。

### Viteで画面を編集する

1つ目のターミナルでAPIを起動する。

```bash
./target/release/amitoki web --config ./amitoki.debug.example.toml
```

2つ目のターミナルでViteを起動する。

```bash
npm --prefix web run dev
```

`amitoki web`が表示したURLのポートを`8710`から`5173`へ変え、`#`以降のトークンを残して開く。例: `http://127.0.0.1:5173/#<一時トークン>`。ReactコンポーネントとTailwindの変更を反映できる。APIのポートを変えた場合は`AMITOKI_WEB_BACKEND=http://127.0.0.1:8711 npm --prefix web run dev`で指定する。認証は開発時も有効。

### 検証

```bash
npm --prefix web run format:check
npm --prefix web run check
bash scripts/build-telemetry.sh
bash scripts/build-telemetry-rewrite.sh
npm --prefix web exec -- playwright install chromium
npm --prefix web test
npm --prefix web run test:dev
```

通常のテストはRustに埋め込んだ画面を検証する。`test:dev`は一時コピーでViteを起動し、PCAP再生とHMR後の選択状態維持を検証する。作業中のソースは書き換えない。既定のバイナリは`target/release/amitoki`。`AMITOKI_BINARY=target/debug/amitoki`で切り替えられる。

画面は`web/src`の`packets`・`pipeline`・`operations`、API型/通信は`api`、取得/監視のhookは`state`。色と共通部品は`app.css`、個別の配置はTSXのTailwindクラス、表示文言は`labels.ts`に置く。構成は[ReactのTypeScriptガイド](https://react.dev/learn/typescript)、[Vite](https://vite.dev/guide/)、[TailwindのVite連携](https://tailwindcss.com/docs/installation/using-vite)を参照。

# amitoki（あみとき）

v0.2.0では[Rust Pipeline/Stage・生成テスト・reload](docs/rust-stages-reload.md)に対応します。

ネットワークの解析・デバッグ・通信実験に使うRust製ツール。Ethernetフレームを解析・フィルタリングし、設定で選んだ中継方式を通して別ノードへ送る。PostgreSQL・P2Pは外部プロセス型プラグインとして追加する。本体の再ビルドは不要。メモリ中継は同一プロセス内のテスト用として組み込んでいる。

0.1.0では解析・フィルタも外部ブロックとして作成し、複数の中継先へ分岐できる。[ブロック構成の設計・使い方](docs/block-pipelines.md)と[設定例](amitoki.pipeline.example.toml)を参照する。

種類別CLI、GitHub URL・ディレクトリからの追加、PCAPでの単体テスト・経路トレース・結果比較・変更監視は[プラグイン開発とデバッグ](docs/plugin-development.md)を参照する。

開発版は`amitoki web --config ./amitoki.toml`で[Web UI](docs/web-ui.md)を開ける。PCAP解析、Stageごとの入出力、パイプライン図、本体・Relayの稼働状況をローカルのブラウザに表示する。

開発版では中継ごとの送信キュー・ACK待ちを分離し、停止した経路だけを制限する。[パケット加工Stage](plugins/telemetry-rewrite/README.md)と[耐久・障害試験](docs/endurance.md)も利用できる。

Linux向け。本体の実行ファイル・debの入手方法とCIは[インストールとリリース](docs/releases.md)、ソースからのビルドは以下を参照する。プラグインを追加する場合は[中継プラグインの設計](docs/relay-plugins.md)、変更点と検証範囲は[監査・検証記録](docs/review-2026-09-26.md)を参照する。

[stegrdbのfeat/relay-plugins](https://github.com/aida0710/stegrdb/tree/feat/relay-plugins)の履歴を引き継いだ独立プロジェクト。名前は「網＋解き」。移行手順は[amitokiへの移行](docs/amitoki-migration.md)を参照する。

## ビルドする

Ubuntu/Debianで必要なツールを入れる。Rustの導入方法は[rustup公式](https://rust-lang.github.io/rustup/installation/other.html)に従う。ソースからのビルドにはNode.js 24以降も必要。[Web開発の手順](docs/web-ui.md#開発環境)で準備する。配布バイナリの実行にNode.jsは不要。

```bash
sudo apt-get update
sudo apt-get install -y build-essential git curl ca-certificates libcap2-bin postgresql-client python3
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
rustup component add rustfmt clippy
git clone https://github.com/amitoki/amitoki.git
cd amitoki
bash scripts/build-web.sh
cargo build --release --bin amitoki --locked
```

`./setup.sh`でも設定ファイルの作成とreleaseビルドを行える。既存の設定ファイルを上書きしない。

## プラグインを追加する

PostgreSQLとP2Pはamitoki Organizationの公開リポジトリで開発し、本体からsubmoduleとして参照する。本体だけのビルド・試験にはsubmoduleの取得は不要。開発・VM試験で使う場合は次で取得する。

```bash
git submodule update --init --recursive
```

利用時はプラグインのソース取得やビルドをせず、GitHub Releaseの配布物を追加できる。公式プラグインは公開されているため、GitHubトークンは不要。

```bash
./target/release/amitoki plugin relay add https://github.com/amitoki/amitoki-plugin-postgres
./target/release/amitoki plugin relay add https://github.com/amitoki/amitoki-plugin-p2p
./target/release/amitoki plugin relay list
./target/release/amitoki plugin relay describe postgres
./target/release/amitoki plugin relay configure postgres --set max_connections=4
./target/release/amitoki plugin relay validate postgres
```

`configure postgres`だけなら対話設定になる。環境変数名には`AMITOKI_POSTGRES_URL`、接続数には`4`、初回再生期間には`4000`を入力するか、空欄で既定値を使う。秘密情報の値は入力せず、参照する環境変数名を指定する。

保存先は`$XDG_DATA_HOME/amitoki/plugins`、未設定なら`~/.local/share/amitoki/plugins`。サービス用には`AMITOKI_PLUGIN_DIR`で共通の場所を指定する。CLIの`--directory`はその操作に限った指定なので、サービス起動にも同じ場所を設定する。プラグインの設定は`.config/<名前>.json`へ権限0600で保存し、`[relay.options]`の項目で上書きできる。

```bash
./target/release/amitoki plugin relay update p2p
./target/release/amitoki plugin relay del p2p
./target/release/amitoki plugin relay add ./dist/postgres
```

Relayの設定保存・更新・削除は対象の中継を停止してから行う。使用中のRelayへの操作は拒否する。Stageは稼働中に更新でき、reloadで反映する。削除後も設定は保持する。更新は実行ファイルのSHA256、通信仕様、OS・CPU、保存済み設定を検証し、原子的に入れ替える。SHA256は破損検出であり、第三者署名ではない。取得先リポジトリと認証済みHTTPSを信頼境界とする。

P2Pの鍵作成・接続設定・任意のNext.js接続情報交換サーバは[amitoki-plugin-p2p](https://github.com/amitoki/amitoki-plugin-p2p)のREADMEを参照する。

## 中継先と対象ネットワークを設定する

`amitoki.example.toml`を`amitoki.toml`へコピーする。各ノードで`node_id`を変え、中継相手とは同じ`channel`を指定する。`interface`には中継対象LANのインターフェースを指定する。DBへ接続するインターフェースは分ける。

フィルタは規定で全拒否。必要な通信だけを`[firewall].rules`へ追加する。各ルールはOR条件で、送信側と受信側の両方で適用される。ARPを中継するには、IPのルールとは別に`EtherType`の2054を許可する。

```toml
[firewall]
policy = "whitelist"
rules = [
  { type = "SrcIpAddress", value = "192.168.50.10" },
  { type = "DstIpAddress", value = "192.168.50.10" },
  { type = "EtherType", value = 2054 },
]
```

PostgreSQLの接続情報は`AMITOKI_POSTGRES_URL`に設定する。接続文字列はlibpq形式またはPostgreSQL URIを使える。次の入力はターミナルへ表示されない。入力例の形式は`host=db.example.com user=amitoki password=... dbname=amitoki sslmode=require`。実際の接続先と認証情報へ置き換える。

```bash
read -r -s -p 'PostgreSQL接続文字列: ' AMITOKI_POSTGRES_URL
printf '\n'
export AMITOKI_POSTGRES_URL
~/.local/share/amitoki/plugins/postgres/amitoki-plugin-postgres --schema > schema.sql
psql "$AMITOKI_POSTGRES_URL" --set ON_ERROR_STOP=1 -f schema.sql
```

初期化SQLはスキーマを作成できる権限で実行する。既存のパケットログ用テーブルは変更しない。実行時は専用スキーマへの必要な読み書き権限を持つユーザを使う。接続文字列は`.env`に保存してもよい。保存する場合は`chmod 600 .env`を適用する。

TLSでサーバ証明書とホスト名を確認する。プライベートCAを使う場合はOSの信頼ストアへ登録する。暗号化しない接続を使う隔離試験では、`sslmode=disable`を明示する。

## 設定を確認して起動する

```bash
./target/release/amitoki --list-plugins
./target/release/amitoki --config amitoki.toml --check-config
sudo setcap cap_net_raw=ep ./target/release/amitoki
./target/release/amitoki --config amitoki.toml
```

`--check-config`は共通設定と外部プラグインの設定スキーマを検証する。DBへの接続やインターフェースの存在は実行時に確認する。再ビルド後は実行ファイルのcapabilityを再設定する。

Ctrl+CまたはSIGTERMで収集を止め、保存待ちのフレームを送ってから終了する。停止期限を超えるとエラーと未保存件数を出す。ログは標準エラーへ出力し、`RUST_LOG`でレベルを変えられる。

## テストする

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p amitoki --no-default-features --locked
cargo bench --bench packet_pipeline --locked
```

外部サービスを使うテストは通常の`cargo test`ではスキップする。Dockerが未導入のUbuntu/Debianでは次のように準備できる。Dockerを実行できる権限のあるターミナルからテストを実行する。

```bash
sudo apt-get install -y docker.io
sudo systemctl enable --now docker
bash scripts/test-postgres.sh
bash scripts/test-network.sh
```

PostgreSQL試験は一時コンテナを作り、終了時に削除する。ネットワーク試験は外部接続のないコンテナ内に3組のvethを作る。ホストのインターフェースを変更しない。Dockerのテスト用イメージはキャッシュとして残る。

VMで起動から通信まで確認する場合は[3台のVMによるテスト環境](docs/vm-lab.md)を使う。`scripts/vm-lab up`で環境を作り、`scripts/vm-lab test`でICMP・TCP・UDPとノード停止後の再配送を検証する。

## 旧版から移行する

設定は`.env`とDBの制御テーブルからTOMLへ移した。旧`NODE_ID`は`node_id`、`DOCKER_INTERFACE_NAME`は`interface`として指定する。旧`TIMESCALE_DB_*`は一つの接続文字列へまとめる。対話式のインターフェース選択はなく、設定で指定する。

`node_list`と`node_activity`は新しい実行経路では使わない。`firewall_settings`のルールはTOMLへ移す。同じpolicyのルールは従来どおりOR条件で、優先度0も有効な通常ルールとして扱う。IPやポートを持たないパケットに、架空の0番ポートを割り当てない。

旧`packets`と`processed_packets`からの自動移行は行わない。新しい`stegrdb_relay`スキーマを作り、相手ノードも同じ版へそろえる。保存された時刻間隔を再現する待機もなくし、取得できたフレームから順に送る。旧IDPS専用ログ設定は使わず、標準エラーのログと終了時の集計を見る。

PostgreSQLを組み込んでいたstegrdbから移行する場合は、`plugin relay add https://github.com/amitoki/amitoki-plugin-postgres`でプラグインを追加する。接続設定とDBスキーマはそのまま使える。amitoki本体はPostgreSQLに依存せずにビルドされる。

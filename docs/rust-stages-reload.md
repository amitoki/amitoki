# RustでPipelineとStageを開発する

Stageは1つの解析・フィルタ処理、PipelineはStageとRelayの接続関係、Pluginはその実装を配布する単位です。既存のblockはStageに対応します。CLIは`plugin stage`、既存の`plugin block`も使用できます。配布形式とIPCの`block`フィールドは既存版との通信を維持しています。

## Rustから接続を定義する

`amitoki-pipeline`の`Pipeline`、`Stage`、`RelayInstance`を使用します。SDKからも`amitoki_plugin_sdk::pipeline`として参照できます。登録名によって外部プラグインを指定するため、本体への静的リンクは必要ありません。

```bash
cargo run --example pipeline -- ./pipeline.json
```

[examples/pipeline.rs](../examples/pipeline.rs)は、解析→フィルタ→ログの3段を生成します。接続順は`connect`で指定します。分岐は複数の接続先、破棄は`discard`で表します。順序変更には接続を変更してください。Stageの登録順だけを変えても接続順は変わりません。

運用設定には生成物への参照を書きます。相対パスは運用設定のディレクトリ基準です。絶対パスと`~/`も使用できます。

```toml
node_id = "node-a"
channel = "lab"
interface = "eth0"
pipeline_file = "pipeline.json"

[firewall]
policy = "whitelist"
rules = [{ type = "EtherType", value = 34997 }]
```

`pipeline_file`とインラインの`[pipeline]`、旧`[relay]`は排他です。Rustの生成器は一時ファイルをrenameして出力します。本体が読み込む際に、ポート、循環、到達可能性、分岐上限とループ抑制の制約を検証します。生成物はJSONで1MiB以内です。Rustソースを本体で実行したり、ゲスト上でコンパイルしたりはしません。

サンプルのRelayは試験用memoryです。複数ノードの通信には、登録済みのPostgreSQL/P2Pとその設定へ変更してください。

## パケット定義とStage

`amitoki-packet::PacketCodec`に`decode`と`encode`を実装して、バイト順・長さを定義します。Rustのstructをそのままメモリコピーして通信に使いません。生成用の値・分布は別の`PacketGenerator`に実装します。同じcrateをStageと生成器から参照できます。

参考実装の`Telemetry`は実験用EtherType 0x88b5で、形式識別子・版・連番・温度・可変長ペイロードを持ちます。`telemetry-v1`生成器はseedと入力番号から再現可能なパケットを生成します。decodeでは不正長、版、予約フィールド、パディングを拒否します。

```bash
bash scripts/build-telemetry.sh
target/release/amitoki plugin stage add ./dist/telemetry
target/release/amitoki plugin stage describe telemetry
target/release/amitoki plugin stage configure telemetry --set threshold=80
```

Stage実装は`amitoki_plugin_sdk::stage::{Stage, StagePlugin, StagePacket, StageOutput}`を使います。Stageは出力ポートとannotations、加工を宣言した場合は任意のbytesを返します。本体が加工後の検証と配送IDの生成を行います。[加工Stageの例](../plugins/telemetry-rewrite/README.md)を参照してください。生成器はデバッグ専用です。

配布用`plugin.json`は`--describe`のRust定義から`scripts/package-plugin.py`で生成します。パケット生成器はmanifestの`packets`に名前と生成条件のJSON Schemaを公開し、`StagePlugin::generate`で実装します。テスト専用パケット定義を提供する別プラグインを`--generator`で参照することもできます。

## 生成テスト・負荷測定

```bash
amitoki plugin stage test telemetry --packet telemetry-v1 --count 1000 --seed 42 --json
amitoki plugin stage test telemetry --packet telemetry-v1 --packet-set payload_bytes=1400 --set operation=filter
amitoki plugin stage bench telemetry --packet telemetry-v1 --count 100000 --batch-size 128 --json
amitoki plugin stage bench telemetry --packet telemetry-v1 --duration 30s --rate 10000 --json
amitoki plugin stage watch ./dist/telemetry --packet telemetry-v1 --count 1000 \
  --watch ./plugins/telemetry --watch ./crates/packet --build ./scripts/build-telemetry.sh
```

`test`は既存の`--pcap`と`--packet`のどちらかを指定します。ローカル配布ディレクトリは一時ストアで使い、登録済みの設定を変更しません。生成バッチは最大128件です。`--packet-set`は生成器の設定、`--set`はテスト対象Stageの設定です。別の生成器にはテスト対象の設定を渡しません。

`bench`は生成パケットを本体の入力検査・プランナー・実際の外部Stageへ渡します。NICやRelayは起動しません。生成パケットの拒否やStageエラーは終了コードを失敗にします。ウォームアップは既定128件で測定件数に含まず、Stage内部状態はそのまま測定区間へ引き継ぎます。0に変更できます。計測には次の値を出力します。

- 投入したパケット数、バッチ数、出力ポートごとの件数、本体で拒否した件数、エラー数
- 全体の経過時間と件数/秒。生成・IPC・本体検査・指定レートの待機時間も含む
- 生成時間と処理時間。処理時間には入力検査・プランナー・StageへのIPCが含まれる
- バッチ処理時間のp50/p95/p99。固定長log2ヒストグラムの区間上限で、パケット単体の遅延ではない

`--rate`はバッチ単位の投入間隔です。1件ずつ投入する場合は`--batch-size 1`を指定します。処理が間に合わない場合の実測レートは指定値より低くなります。`--duration`終了時には処理中のバッチを完了させるため、経過時間は指定時間を超える場合があります。Stage単体の測定であり、ネットワーク全体の帯域を測るものではありません。

## reload

```bash
# Stageの配布物だけをビルド・更新
amitoki plugin stage update telemetry --path ./dist/telemetry
# 必要ならRustの接続定義を再生成
cargo run --example pipeline -- ./pipeline.json
# 本体と同じユーザで、稼働時と同じ運用設定を指定
amitoki reload --config ./amitoki.toml
```

SIGHUPも同じ処理を実行します。VM用systemd unitは`systemctl reload amitoki`に対応します。CLIの成功は新世代への切り替え完了を示し、旧世代の全処理完了を待つものではありません。失敗の詳細はCLIと本体ログに出ます。SIGHUPの結果は本体ログで確認してください。

1. 新設定を検証し、新Stageの実行ファイルのコピーを保持して起動・初期化する。
2. すべて成功したら、入口が参照するPipelineの世代を原子的に交換する。
3. 新しくキューへ受け付けるパケットに新世代を割り当てる。
4. 旧世代のパケットは旧接続・旧Stageで処理し、配送・ACKが完了するまで旧世代を保持する。
5. 最後の利用が終わった旧世代のStageを終了する。

パケットを処理途中で新世代へ移しません。配送の再試行でも解析を繰り返しません。解析は共通キューで行い、中継ごとの送信キューとACKの再試行は独立しています。旧世代はその世代の送信待ちやACKが終わるまで保持します。停止した中継によって旧世代の保持上限に達した場合、追加のreloadは拒否されます。

コピー・ハッシュ検証はblocking poolで行い、本体の非同期処理を占有しません。初期化が失敗・期限超過した場合は新世代を破棄し、旧世代を継続します。`engine.reload_timeout_ms`は既定30000、最大300000です。旧世代の完了待ちは最大3世代までで、それ以上のreloadは旧世代が完了するまで拒否します。

### 維持する状態と変更範囲

Stageの追加・削除・設定・実装、Pipelineの接続関係を変更できます。Stage内部のカウンタや解析状態は初期化します。状態の移行は未対応です。フロー途中の状態が必要なStageでは、再初期化の影響を確認してください。

本体のNIC、Relay接続、配送ID、ACK、キュー、重複抑制履歴、メトリクスは維持します。NIC・Relay一覧/設定・node_id/channel・firewall・Engine設定の変更は拒否し、再起動を必要とします。パケット損失ゼロやクラッシュをまたぐexactly-onceを保証するものではありません。

Stageは起動時のコピーを使うため、稼働中でもインストール先の更新・設定変更・削除が可能です。削除しても稼働中のStageは直ちには止まりません。次のreloadで参照先が存在しなければ失敗して旧構成が続きます。Relayの使用中ロックは維持します。

制御ソケットは同じUIDのみ接続可能な`/tmp/amitoki-control-UID/`に作成し、設定ファイルの絶対パスごとに識別します。同じ設定による二重起動を拒否します。新しいRustコードのコンパイルはプラグインと接続定義の生成器に必要ですが、本体を再ビルドする必要はありません。

## VMでの確認

既存ラボでは、まず1台へ新しい本体を配備してreloadを試せます。3台の通信相手は起動しておいてください。

```bash
scripts/vm-lab up a --relay both --pipeline
scripts/vm-lab reload-test a
# 1台で確認できたら、同じビルドを3台へ配備する
scripts/vm-lab up --relay both --pipeline
scripts/vm-lab reload-test
scripts/vm-lab test --relay both --pipeline
```

`reload-test`は通信中のStage追加・実装更新、不正な設定の拒否、SIGHUPを試し、本体とRelayのPID・本体のSHA256が変わらないことを確認します。各ノードで100件のpingと1000件の生成ベンチを実行し、設定は試験後に復元します。記録は`artifacts/vm-reload/`に保存します。ゲスト側のCargoは不要です。

# telemetry-rewrite

共有Rustパケット定義`telemetry-v1`を加工するサンプルStage。`temperature`で温度を変更し、`redact_payload=true`でペイロードを同じ長さのゼロにする。MACアドレス・連番・長さは維持する。形式が一致しない入力は`drop`へ流す。

```bash
bash scripts/build-telemetry.sh
bash scripts/build-telemetry-rewrite.sh
cargo build --release --bin amitoki --locked
./target/release/amitoki plugin stage test ./dist/telemetry-rewrite \
  --generator ./dist/telemetry --packet telemetry-v1 --count 1000 \
  --set temperature=25 --set redact_payload=true --json
python3 scripts/check-telemetry-rewrite.py
```

最後の検証は、既知のPCAPを加工→解析へ流し、加工後のSHA256と次のStageでの温度25を照合する。CLIのtest/replay/compareには加工後の長さ・SHA256も含まれる。NICや外部Relayは起動しない。

Pipelineに追加する例:

```toml
[[pipeline.stages]]
id = "rewrite"
plugin = "telemetry-rewrite"
[pipeline.stages.options]
temperature = 25
redact_payload = true
```

入力を`rewrite`へつなぎ、`rewrite.pass`を次のStageかRelayへ、`rewrite.drop`を`[]`へつなぐ。送信側・受信側のどちらにも置ける。生成器は別の`telemetry`プラグインを使う。

## 本体との境界

SDK 0.4.0で`StageDefinition.rewrite=true`を宣言し、`StageOutput.bytes=Some(...)`を返す。`None`は入力バイト列の維持。加工結果は出力ポート全体に共通で、1入力につき最大1フレーム。複数の異なる加工を行う場合は分岐先の別Stageへ置く。

本体は各加工結果の長さ・パケット構造・firewallを再検証する。ID・ACK・NIC操作はStageへ渡さない。加工後のIDは元の入力IDと最終バイト列から本体が決める。同じ入力と同じ結果なら再配送でも同じIDとなり、元に戻した場合は入力IDを維持する。NICへの注入は加工後IDで重複を抑制し、取得元へのACKは元のreceiptを使う。

同じ入力の異なる加工結果が同じ終端へ合流するとエラー。同じ結果の合流は1回の配送にまとめる。異なる終端には別の結果を送れる。Stage内部の非決定的な処理、クラッシュ、履歴の追い出し後のexactly-onceは保証しない。

IP/TCP/UDPを書き換える自作Stageでは、チェックサムと関連する長さもStage側で更新する。本体の構造検証はチェックサムの修復を行わない。このサンプルは実験用EtherTypeを使うためIPチェックサムを持たない。新規フレームを任意件数生成するAPIではない。

既存の加工しない配布物はそのまま利用できる。古い本体は`rewrite`宣言を理解できず導入を拒否するため、加工対応の本体が必要。Relayの通信仕様v1は変更していない。Rustソースから再ビルドする既存Stageは定義に`rewrite:false`、出力に`bytes:None`を追加する。

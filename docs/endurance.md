# 中継の独立性・耐久性・性能を検証する

単体試験では送信停止・キュー満杯・ACK待ち・プロセス終了を再現し、他の中継の後続配送が継続することを確認する。停止後の再試行で解析や成功済み配送を繰り返さないこと、受信加工後の重複注入と元receiptへのACK、reload世代の保持も検証する。

```bash
cargo test --locked --test pipeline_runtime
cargo test --locked --lib pipeline
```

## 3台のVMで実通信と障害復旧を繰り返す

[VMラボ](vm-lab.md)を準備し、先に1台へ配備・確認してから3台へ広げる。

```bash
scripts/vm-lab up a --relay both --pipeline
scripts/vm-lab up --relay both --pipeline
scripts/vm-lab test --relay both --pipeline
scripts/vm-lab endurance --duration 300 --fault postgres
```

`endurance`はa→b、b→c、c→aでTCP 2MiBの内容照合とUDP 64件のエコー照合を繰り返す。指定時間の1/3でPostgreSQLを停止し、2/3で復旧する。本体とP2Pを再起動せず通信が続くこと、復旧後にDBの配送待ちが解消することを確認する。例外時もPostgreSQLを復旧する。各操作には接続・実行期限がある。実行時間は周期完了や復旧待ちの分だけ指定値を超える。

`artifacts/endurance/<日付>/<時刻>/result.json`に各周期の転送量・TCPのペイロード速度・UDP照合件数・本体PID・本体と子プラグインのRSS合計を保存する。本体PIDが変わるか、通信内容が一致しなければ失敗。長時間用は`--duration 3600`などを指定する。`--fault none`は障害を入れず測定する。

転送速度には接続確立と応答時間を含む。QEMUの仮想NIC・ホスト負荷・TCP制御・SQL・プラグインIPCの影響を受けるため、物理NICの最大帯域とは区別する。UDPは応答を待ちながら送る機能検証で、最大pps測定ではない。RSSを記録するだけでメモリリーク不存在を判定しない。

## Stage単体の負荷

```bash
bash scripts/build-telemetry.sh
bash scripts/build-telemetry-rewrite.sh
./target/release/amitoki plugin stage bench ./dist/telemetry-rewrite \
  --generator ./dist/telemetry --packet telemetry-v1 \
  --duration 60s --batch-size 128 --warmup 1000 \
  --packet-set payload_bytes=1400 --set temperature=25 --json
```

ペイロード0・64・1400バイト、batch-size 1・32・128を変えて測る。pps・遅延分布・生成時間・処理時間・拒否/エラー数をJSONで比較できる。これはStageとIPC、本体の解析・計画を含む値で、実通信の帯域ではない。

## 障害時の保証範囲

- `engine.relay_queue_capacity`は中継ごとの送信待ち上限。処理中も数え、満杯時はその経路だけ新しいバッチを破棄する。
- 永続的なディスク退避・自動フェイルオーバーはない。中継の受け付け成功と対向NICへの到達は別。
- 再試行可能な接続障害は復旧を待つ。プラグインプロセス終了などの恒久障害は該当方向を停止し、本体再起動で復旧する。
- 共通のNIC障害や`on_error=stop`のStageは全体を停止する。Stage処理は共通executorで実行するため、遅いStageのタイムアウトまでの影響は残る。
- 履歴と送信待ちはメモリ上。クラッシュや履歴上限をまたぐ重複排除は保証しない。

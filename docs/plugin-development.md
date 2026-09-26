# プラグインを追加・開発・デバッグする

`amitoki plugin <種類> <操作> <対象>`で管理する。`relay`は中継、`block`は解析・フィルタ・分岐などの処理。種類は配布物の定義と照合し、間違った種類で追加・更新・削除する操作を拒否する。

## 追加する

```bash
./target/release/amitoki plugin relay add https://github.com/amitoki/amitoki-plugin-postgres
./target/release/amitoki plugin relay add https://github.com/amitoki/amitoki-plugin-p2p --version v0.1.1
./target/release/amitoki plugin block add ./dist/packet-rules
```

GitHub URLはリポジトリのHTTPS URL、`.git`付きURL、`/releases/tag/v0.1.1`形式のRelease URLに対応する。リポジトリURLだけなら最新Release、タグURLまたは`--version`付きなら指定版を取得する。URLと`--version`のタグが異なる場合はエラーになる。git cloneやビルドは行わず、OS・CPUに合うReleaseの配布物を取得し、版・通信仕様・SHA256を検証する。

ディレクトリには`plugin.json`と実行ファイルを置く。ソースコードのディレクトリを自動ビルドする機能ではない。相対パスは実行したディレクトリが基準。絶対パスと`~`・`~/...`にも対応し、引用符の中の`"~/projects/my-block/dist"`もCLIが展開する。`~user`形式には対応しない。

単語だけなら追加済みの名前として扱う。同名のディレクトリを指定するときは`./name`を使う。`plugin block add packet-rules`は追加済みかを表示し、更新しない。未登録名からリポジトリを推測しない。プラグイン名は種類をまたいで一意とする。

## 追加後は名前・URL・パスで指定する

```bash
./target/release/amitoki plugin block describe packet-rules
./target/release/amitoki plugin block configure ./dist/packet-rules --set label=inspection
./target/release/amitoki plugin block validate packet-rules
./target/release/amitoki plugin block update packet-rules
./target/release/amitoki plugin relay update https://github.com/amitoki/amitoki-plugin-p2p --version v0.1.1
./target/release/amitoki plugin block del ./dist/packet-rules
./target/release/amitoki plugin relay list
./target/release/amitoki plugin block list
```

追加時の取得元を保存する。ローカルは絶対パスへ解決し、`update 名前`でその配布物を再読込する。元ディレクトリを消しても、同じ絶対パス・相対パスで登録の削除ができる。シンボリックリンクは追加時に実体へ解決するため、リンクを付け替えた場合は追加済みの名前を使う。

URLによる`describe/configure/validate/del`は保存済みの取得元と照合するだけで、ダウンロードしない。同じ取得元に複数登録がある場合は名前を指定する。これらの操作ではURL内のタグは登録の選択に使わず、リポジトリを照合する。タグを変更するのは`update`だけ。

版を固定した追加では、`update 名前`とタグなしURLによる`update`も固定版を維持する。別の版へ移すときは`--version`かタグURLを指定する。版を固定していない追加は最新Releaseへ更新する。

`del`はインストール先のコピーだけを削除し、元の開発ディレクトリと保存済み設定を残す。稼働中の設定変更・更新・削除は拒否する。`add/del`はインストール管理で、パイプラインへの配置はTOMLで行う。同じプラグインを別設定で複数インスタンスとして使える。

既存の`plugin add postgres`、`plugin add owner/repo@tag`、`plugin add --path DIR`、`plugin remove NAME`も継続して使える。`remove`は`del`、`config`は`configure`の別名。旧版で追加したローカル配布物は取得元が未記録なので、最初の更新に`update NAME --path DIR`を使うと以後のパス指定が有効になる。

保存先は既存どおり`AMITOKI_PLUGIN_DIR`、`XDG_DATA_HOME/amitoki/plugins`、`~/.local/share/amitoki/plugins`の順。`--directory`でそのコマンドだけ上書きできる。

## ブロック単体で試す

最初に本体と参考ブロックをビルドし、配布用ディレクトリと合成PCAPを作る。

```bash
cargo build --release --bin amitoki --locked
cargo build --release -p amitoki-block-packet-rules --locked
python3 scripts/package-plugin.py target/release/amitoki-plugin-packet-rules dist/packet-rules
mkdir -p artifacts/debug
python3 scripts/create-debug-capture.py artifacts/debug/sample.pcap
./target/release/amitoki plugin block test ./dist/packet-rules \
  --pcap artifacts/debug/sample.pcap --set 'allowed_ether_types=[34998]'
```

1件目は`pass`、2件目は`drop`、3件目は不正長なので本体で拒否される。単体テストでは各ポートを観測用の終端`output:<ポート名>`へ接続する。`drop`というポート名自体に特別な意味はない。実際に破棄するかはパイプライン設定が決める。出力ポートを空にするブロックは、その時点で破棄する。

ローカル配布物は一時保存先へコピーして実行し、通常のインストール先を変更しない。追加済みの名前かURLでもテストできる。URLからのテストは追加済み登録を使うため、初回は`add`を実行する。`--set`はそのテストだけの設定上書きで、保存済み設定を書き換えない。ローカル配布物には保存済み設定を引き継がない。

`--node-id`・`--channel`の既定値は`debug`、`--instance`は`test`。通常のパーサで拒否するフレームはブロックへ渡さない。単体テストはユーザのfirewallルールを適用せず、経路の再生では設定ファイルのルールを適用する。

## パイプラインの経路を追う

```bash
./target/release/amitoki plugin block add ./dist/packet-rules
./target/release/amitoki debug replay --config amitoki.debug.example.toml \
  --pcap artifacts/debug/sample.pcap
./target/release/amitoki debug replay --config amitoki.debug.example.toml \
  --pcap artifacts/debug/sample.pcap --source output.received
```

`--source`は既定で`capture`。`<中継ID>.received`を指定すると受信後の経路を試せる。各パケットについて、本体の拒否理由、通ったブロック、出力ポートと接続先、解析結果、処理時間、最終送信先を表示する。

グラフの制約、本体のパーサ・firewall、ブロックの入出力検証、タイムアウト、`stop/drop_branch`、終端での合流は通常実行と同じ処理を使う。同時に実行可能なブロックは設定の記載順で処理し、再生ごとの順序を安定させる。

ブロックは実際に別プロセスで動かす。NICと中継プラグインは起動せず、最終送信先は送信予定として表示する。設定中の中継配布物・設定スキーマの検証は行う。プラグイン自体がファイルや通常のネットワークへアクセスする動作は残るため、OSサンドボックスとしての隔離ではない。

入力は[libpcapのPCAP仕様](https://github.com/the-tcpdump-group/libpcap/blob/master/pcap-savefile.manfile.in)に沿ったclassic PCAP 2.4のEthernet（追加FCS情報なし）に対応し、両エンディアンとマイクロ秒・ナノ秒の時刻を読める。PCAPNG、Linux cooked capture、保存時に切り詰められたパケットは再生対象外。壊れたPCAPはエラー、切り詰められたレコードは本体の拒否として報告する。

ファイル順に1パケットずつ再生し、記録時刻の間隔では待たない。IDはレコード番号から固定する。本番のバッチ境界・配送ID・ACK・再配送・NICの重複排除は再現しない。処理時間はIPCと待機を含み、最大帯域の測定値ではない。

## 修正前後を比較する

```bash
./target/release/amitoki plugin block test ./dist/packet-rules \
  --pcap artifacts/debug/sample.pcap --set 'allowed_ether_types=[34998]' \
  --json > artifacts/debug/before.jsonl
./target/release/amitoki plugin block test ./dist/packet-rules \
  --pcap artifacts/debug/sample.pcap --set 'allowed_ether_types=[]' \
  --json > artifacts/debug/after.jsonl
./target/release/amitoki debug compare artifacts/debug/before.jsonl artifacts/debug/after.jsonl
```

この例は2件目の出力が変わるので終了コード1になる。差分なしは0。実装を修正した場合もプラグインだけを再ビルド・再パッケージして同じ入力で試す。本体の再ビルドは不要。

`--json`は1パケット1行のJSONLをstdoutへ出す。処理時間を除く結果を入力順に比較し、入力のSHA256・時刻・拒否理由・経路・解析結果・最終送信先・エラー・件数の違いを検出する。ブロック内で現在時刻や乱数を解析結果に入れれば差分になる。結果には解析した内容が含まれるので、公開する前に内容を確認する。

今回の範囲はブロック単体と経路の検証。中継の実通信・再配送は[VM試験](vm-lab.md)で確認する。自動ビルドを行う`watch`、PCAPNG、GUIによる編集、ホットリロードは未実装。

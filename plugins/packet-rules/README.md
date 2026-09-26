# packet-rules

解析・フィルタブロックの参考実装。独立した実行ファイルなので、設定と実装を変えてビルドし直しても本体を再ビルドする必要はない。

- `allowed_ether_types`: 外側のEtherTypeの許可一覧。空は全通過。
- `label`: 解析結果に付ける文字列。
- `log_every`: N件ごとにstderrへ処理件数を出す。0は無効。
- 出力ポート: `pass`、`drop`。

入力の解析結果を保持し、インスタンスIDをキーとして`ether_type`・`length`・`label`を追加する。VLANの内側やTCP/UDPの意味までは解釈しない。独自の解析処理に置き換える例として使う。

ビルド・パッケージ化・接続は[ブロック構成の手順](../../docs/block-pipelines.md)を参照する。この例は本体リポジトリ内のworkspaceメンバーとして管理する。別リポジトリで開発する場合はSDKをGit revisionで固定し、同じ配布形式でCLIから追加できる。

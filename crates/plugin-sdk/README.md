# amitoki-plugin-sdk

本体を再ビルドせずに中継方式を追加するためのRust SDK。プラグインは`serve`へ実装と`PluginManifest`を渡す。配布物はOS・CPU別の実行ファイルと設定定義を含むmanifestで、本体はRust ABIに依存しない。

通信仕様v1はstdin/stdout上の「4バイトのbig-endian長 + MessagePack」。stdoutをログへ使わない。要求と応答は1対1・同時に1件で、describe/connect/publish/receive/acknowledgeを使う。起動時に通信仕様・名前・バージョン・設定スキーマを照合する。

フレームはUUIDとバイト列、受領情報は不透明な文字列。1バッチ128件、1メッセージ16MiBを上限にする。receiveは非破壊、ACKは冪等。プラグインは配送保証、保持期間、クラッシュ時の扱いを文書化する。

本体側の呼び出しキャンセル後も専用タスクが応答を読み終え、次の要求との混同を防ぐ。IPCが30秒以内に完了しない場合やプロセス終了時は接続を破棄し、中継サービスの再起動を要求する。プラグインの状態を失う自動再起動は行わない。

設定定義はJSON Schema。必須項目・型・既定値・説明をプラグイン側が持つ。値を含む検証エラーは表示しない。ネットワーク接続前に共通の検証を行い、実装固有の検証はconnectでも行う。


Stage API・共有Rustパケット定義・Pipeline生成は[開発手順](../../docs/rust-stages-reload.md)を参照してください。Stageの配布メタデータはRustのmanifestから生成します。

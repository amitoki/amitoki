# stegrdbからamitokiへ移行する

amitoki（あみとき）は、ネットワークの解析・デバッグ・通信実験に使うツール。`aida0710/stegrdb`の`feat/relay-plugins`（`a5110a27304d65e9065540cb288a37d808cd3433`）までの履歴を引き継いで独立した。既存のstegrdbリポジトリはそのまま残す。

## リポジトリ

| 用途 | 公開リポジトリ | amitoki名の配布版 |
|---|---|---|
| 本体・共通SDK | https://github.com/amitoki/amitoki | 本体v0.4.0 |
| PostgreSQL | https://github.com/amitoki/amitoki-plugin-postgres | v0.1.2 |
| P2P・接続情報交換サーバ | https://github.com/amitoki/amitoki-plugin-p2p | v0.1.1 |

プラグインは既存リポジトリをOrganizationへ移管して公開する。stegrdb名のPostgreSQL v0.1.1、P2P v0.1.0とそれ以前のタグ・配布物は保持する。新しい版にはamitoki名の実行ファイルを含める。公式プラグインの取得にGitHubトークンは不要。

旧stegrdb本体を継続する場合は`stegrdb plugin add postgres@v0.1.1`と`stegrdb plugin add p2p@v0.1.0`で旧版を指定する。最新版はamitoki用の実行ファイル名なので、旧本体からバージョンを指定せずに追加・更新しない。

## 設定とプラグイン

1. 既存の中継プロセスを停止する。
2. 新しい本体を[README](../readme.md)の手順でビルドする。
3. 既存のTOMLを`amitoki.toml`へコピーする。`node_id`、`channel`、フィルタ、`relay.options`の構造は同じ。
4. 新しい保存先へプラグインを追加する。

```bash
./target/release/amitoki plugin add postgres
./target/release/amitoki plugin add p2p
./target/release/amitoki plugin configure postgres
./target/release/amitoki --config amitoki.toml --check-config
```

| stegrdb版 | amitoki版 |
|---|---|
| `stegrdb.toml` | `amitoki.toml` |
| `STEGRDB_PLUGIN_DIR` | `AMITOKI_PLUGIN_DIR` |
| `~/.local/share/stegrdb/plugins` | `~/.local/share/amitoki/plugins` |
| `$XDG_DATA_HOME/stegrdb/plugins` | `$XDG_DATA_HOME/amitoki/plugins` |
| `STEGRDB_GITHUB_TOKEN` | `AMITOKI_GITHUB_TOKEN`（privateの取得やAPI制限対策で使う場合のみ） |
| `STEGRDB_POSTGRES_URL`（PostgreSQL既定値） | `AMITOKI_POSTGRES_URL` |

CLIで保存した設定は自動コピーしない。既存の`.config/<名前>.json`を参照して`plugin configure`で設定するか、その内容をTOMLの`relay.options`へ移す。旧インストールディレクトリを新しい保存先として流用せず、amitoki名の配布物を追加する。

PostgreSQLの`connection_env`とP2P discoveryの`token_env`は任意の環境変数名を指定できる。既存の環境変数を継続する場合は、元の名前を設定へ明示する。サーバ側の`SIGNALING_TOKEN`と`UPSTASH_REDIS_REST_*`は変更しない。

サービスのExecStart、Environment、設定ファイルへのパスもamitoki側へ変更する。raw socket用capabilityは新しい実行ファイルへ設定する。VMラボは新しい`amitoki.service`と`amitoki-network.service`を使う。

## 維持する保存形式・通信仕様

- PostgreSQLのスキーマ名は`stegrdb_relay`のまま。既存の未ACKキューを引き継ぎ、改名に伴うDB変換は不要。
- 外部プラグインのMessagePack通信仕様v1は同じ。配布物の実行ファイル名は`amitoki-plugin-<名前>`。
- P2Pの証明書名`stegrdb.invalid`、ALPN `stegrdb-p2p/1`は既存の証明書とピアとの通信のため維持する。
- P2Pのノード占有ディレクトリ`/tmp/stegrdb-p2p-<uid>`とRedisキーの`stegrdb:room:`も共有し、多重起動・ID衝突を同じ範囲で検証する。

過去の検証文書には、実施時点のstegrdb名・privateリポジトリ・コミットをそのまま記載している。現在の名称と配置はこの文書を参照する。

## 移行時の検証（2026-09-26）

Rust・Nodeの試験44件、fmt・Clippy、Next.js production buildが成功した。GitHubの認証情報を外してsubmodule込みのclone、公式Releaseからの追加・バージョン指定更新・設定検証・削除を確認した。旧stegrdb本体でもPostgreSQL v0.1.1とP2P v0.1.0の取得が成功した。

Ubuntu 24.04のVMを3台新規作成し、PostgreSQL・P2Pの両方式でa→b、b→c、c→aのICMP各3回、TCP各2MiB、UDP各64件の到達と内容一致を確認した。ノード停止中のUDP64件も再起動後に届き、使用中のプラグイン更新・削除を拒否した。プラグイン操作前後の本体SHA256は一致し、ゲストにはCargoを入れていない。

ローカル試験記録: `artifacts/vm/2026-09-26/194007`（postgres）、`artifacts/vm/2026-09-26/194051`（p2p）。公開取得試験は`artifacts/migration/2026-09-26/public-install.json`。試験記録はGitから除外している。

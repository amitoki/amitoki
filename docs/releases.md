# 本体のインストールとリリース

本体はLinuxのx86_64・ARM64向けに、実行ファイルを含むtar.gzとdebを配布する。CIはUbuntu 22.04でネイティブビルドし、同じCPU上で両形式をテストする。glibc環境向けで、Alpine Linuxのmusl向けではない。

版タグを付ける前の開発版も、[GitHub Actions](https://github.com/amitoki/amitoki/actions/workflows/rust.yml)の成功した実行から`release-<target>`というArtifactをダウンロードして試せる。ArtifactのZIPを展開するとtar.gz・deb・CPU別のSHA256SUMSが入っている。

## 公開版をインストールする

[GitHub Releases](https://github.com/amitoki/amitoki/releases)から、使用するCPUに合うファイルと`SHA256SUMS`を取得する。以下はv0.4.0のインストール例。GitHub CLIを入れたUbuntu/Debianのターミナルで実行する。

```bash
sudo apt-get update
sudo apt-get install -y gh ca-certificates
mkdir -p amitoki-download
cd amitoki-download
gh release download v0.4.0 --repo amitoki/amitoki \
  --pattern 'amitoki_0.4.0_amd64.deb' --pattern SHA256SUMS
sha256sum --ignore-missing --check SHA256SUMS
sudo apt-get install -y ./amitoki_0.4.0_amd64.deb
amitoki --version
```

ARM64ではファイル名の`amd64`を`arm64`に置き換える。公開リポジトリの取得には認証情報を必要としない。GitHub CLIが認証を要求する環境では、Releaseページから同じ2ファイルをダウンロードし、チェックサム確認以降を実行する。

debは`/usr/bin/amitoki`と`/usr/share/doc/amitoki/`へ本体・説明・設定例を置く。プラグインは別途追加する。ユーザの設定は変更せず、サービスの起動とNIC用capabilityの付与は自動で行わない。PCAPデバッグにはNIC権限は不要。通信に使う場合はREADMEの設定確認手順に従ってから、次のように起動する。更新で実行ファイルが置き換わった場合はcapabilityを再設定する。

```bash
cp /usr/share/doc/amitoki/amitoki.example.toml ./amitoki.toml
# amitoki.tomlのinterface・node_id・中継・フィルタを編集してから実行する。
amitoki --config ./amitoki.toml --check-config
sudo apt-get install -y libcap2-bin
sudo setcap cap_net_raw=ep /usr/bin/amitoki
amitoki --config ./amitoki.toml
# アンインストール。ユーザのプラグインと設定は保持する。
sudo apt-get remove amitoki
```

実行ファイルだけを使う場合は、`amitoki-0.4.0-x86_64-unknown-linux-gnu.tar.gz`を取得・SHA256確認して展開する。ARM64では`aarch64-unknown-linux-gnu`を選ぶ。

```bash
gh release download v0.4.0 --repo amitoki/amitoki \
  --pattern 'amitoki-0.4.0-x86_64-unknown-linux-gnu.tar.gz' --pattern SHA256SUMS --clobber
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf amitoki-0.4.0-x86_64-unknown-linux-gnu.tar.gz
./amitoki-0.4.0-x86_64-unknown-linux-gnu/amitoki --version
```

`SHA256SUMS`はファイルの破損・取り違えを検出するためのもの。配布元の真正性はGitHubリポジトリとHTTPSを信頼する。tar.gzの`build-info.json`とdebの`/usr/share/doc/amitoki/build-info.json`でソースcommit・版・CPU・Rust版・本体のSHA256を確認できる。

## CIの構成

push・PR・手動実行で次を検証する。ワークフローは[`.github/workflows/rust.yml`](../.github/workflows/rust.yml)。Rustは`rust-toolchain.toml`、依存関係は`Cargo.lock`で固定する。

- fmt・Clippy・隔離コンテナでのveth通信とプラグイン権限検証。
- x86_64・ARM64でworkspace試験と`--no-default-features`試験。
- 両CPUのtar.gz・deb生成、ELFのCPUと版、ファイル間の本体SHA256の一致確認。
- 展開した実行ファイル、debをインストールした実行ファイルでプラグイン追加・設定・単体テスト・watch・経路再生・比較・削除。deb自体の削除も確認。
- 配布物の取り違え・公開済み版の上書き・不完全なアップロードを拒否するPython試験。

通常の実行は読み取り権限だけを持ち、Artifactを14日間保持する。版タグのpushでは、全ジョブ成功後に限って公開ジョブへ書き込み権限を渡す。mainに含まれないcommit、Cargo.tomlと違うタグ、不足した配布物を拒否する。

## ローカルで配布物を作る

Python 3.11以上、Rust、dpkg-devが必要。Ubuntu 24.04以降なら次で準備できる。

```bash
sudo apt-get update
sudo apt-get install -y build-essential python3 dpkg-dev
cargo build --release --bin amitoki --locked
bash scripts/build-packet-rules.sh
python3 scripts/release/package.py --target x86_64-unknown-linux-gnu
python3 scripts/release/verify.py dist/release --target x86_64-unknown-linux-gnu
mkdir -p artifacts/release-extracted
tar -xzf dist/release/amitoki-*.tar.gz -C artifacts/release-extracted --strip-components=1
python3 scripts/release/smoke.py --binary artifacts/release-extracted/amitoki --plugin dist/packet-rules
python3 -m unittest discover -s tests/release -v
```

ARM64上では`--target aarch64-unknown-linux-gnu`を使う。クロスビルド用のオプションではなく、異なるCPUの実行ファイルは拒否する。ローカルで新しいglibcを使ってビルドした場合は、必要なglibcの版も上がり得る。公式配布にはCIのビルドを使う。

作業ツリーに変更があるビルドは`dirty: true`を記録する。手元の試用はできるが、公開時は拒否する。アーカイブとdebの時刻・所有者・権限をそろえ、同じビルドからのパッケージ生成を再現可能にする。異なるOSやRust版を使ったコンパイル結果の一致は保証しない。

## 管理者が版を公開する

開発PRをmainへマージした後、[Prepare release](https://github.com/amitoki/amitoki/actions/workflows/prepare-release.yml)をmainから実行し、`version`に次の版を指定する。ターミナルからも開始できる。以下は次の版を0.4.0にする例。

```bash
gh workflow run prepare-release.yml --repo amitoki/amitoki --ref main -f version=0.4.0
```

本体のCargo.toml/Cargo.lock・インストール例を更新し、注釈タグとmainを一緒にpushする。その後、タグの`CI and release`を明示的に起動する。両CPUの検証・配布物生成・正式公開・匿名ダウンロードによる再検証まで自動で進む。公開結果は`CI and release`の対象タグの実行で確認でき、検証結果のJSONは`published-release-verification` Artifactに残る。`Prepare release`の成功は公開CIの開始を意味する。

`docs/releases/v<版>.md`を事前にmainへ追加してあれば、その文章を使う。なければGitHubの変更履歴から自動生成する。SDK・プラグインの版は変更しない。mainの更新競合や既存タグ、別の版の公開CIが進行中の場合は停止する。個人トークンの登録は不要。mainへの直接pushを禁止する保護ルールがある場合は、ルールを迂回せず失敗する。

実公開せずに確認するには、`dry_run=true`を指定する。生成予定の版と変更対象を表示し、commit・タグ・Releaseは作らない。

```bash
gh workflow run prepare-release.yml --repo amitoki/amitoki --ref main \
  -f version=0.4.0 -f dry_run=true
```

タグ作成後にCIの起動だけ失敗した場合は、そのタグを指定して再開する。タグを作り直す必要はない。

```bash
gh workflow run rust.yml --repo amitoki/amitoki --ref v0.4.0
```

## 手元で版を準備する場合

1. `Cargo.toml`の本体版と`docs/releases/v<版>.md`を更新し、PRでCIを通す。SDK・プラグインの版は必要な変更があるときだけ更新する。
2. 関連する開発PRをmainへ取り込む。mainのCI成功と両CPUの配布物を確認する。
3. クリーンなmainで版タグを作成してpushする。

```bash
git switch main
git pull --ff-only
git status --short
# 出力が空であることと、対象commitのCI成功を確認してから実行する。
git tag -a v0.4.0 -m 'amitoki 0.4.0を公開'
git push origin v0.4.0
```

タグのCIは再度ビルド・テストし、draftのReleaseへ4配布物と`SHA256SUMS`をアップロードする。GitHub上のファイル名・SHA256が全部一致したら公開し、認証なしで取得して版・CPU・commit・SHA256・リリースノートを確認する。途中で失敗したdraftは、原因を修正したうえで同じワークフローを再実行できる。公開済み版は上書きせず、配布物とノートが完全に一致する場合だけ再確認へ進む。コード修正が必要なら新しい版を作る。

公開スクリプトに不具合があった場合は、mainで修正して`Resume verified release`を実行する。既存タグのCIで静的チェック・ネットワーク試験・両CPUのテストと配布物生成が成功していることを確認し、そのArtifactを使って公開だけを再開する。タグと配布物は変更しない。Artifactの保存期間は14日。

```bash
gh workflow run publish-release.yml --repo amitoki/amitoki --ref main \
  -f tag=v0.4.0 -f run_id=対象のタグCIのrun_ID
```

再開時はタグのソースへmainの検証・公開スクリプトを適用する。本体版・リリースノートはタグから読み、配布物のcommitがタグと一致することと、タグがmainに含まれることを再検証する。公開済みReleaseは再開でも上書きしない。

`CI and release`の通常ブランチへのpushや、ブランチを指定した手動実行ではReleaseを公開しない。`v`で始まる版タグを指定した手動実行は公開まで進む。これは`GITHUB_TOKEN`で作ったタグのpushが別CIを起動しないためで、Prepare releaseからの連携にも使う（[GitHub公式の起動仕様](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)）。

# makefile-fmt

GNU Makefile の意味を変えないことを優先した、保守的な formatter です。
v0.1 の意味保存の保証基準は **GNU Make 4.4.1** です。3.x 系を含む古い GNU Make との互換性は保証しません。

安全に認識できる単純な変数代入と TAB recipe を整形します。未知の構文や安全に処理できない command は原文を保持します。保証の前提と検出対象の詳細は [MVP.md](MVP.md) を参照してください。

## ビルドと使用方法

Rust / Cargo と、`--explicit-semicolons` に対応した fork 版 `shfmt` が必要です。開発時の検証には Rust 1.96.0 を使用しています。`shfmt` を PATH に置いてください。

```sh
cargo build --release
./target/release/makefile-fmt Makefile
./target/release/makefile-fmt --check Makefile
./target/release/makefile-fmt --diff Makefile
./target/release/makefile-fmt -w Makefile
```

デフォルトは整形結果を stdout に出力します。`--check` は差分の有無を終了コードで返し、`--diff` は unified diff を出力します。`-w` は元のファイルを更新します。一度に指定できる入力は通常ファイル1件で、モードの併用はできません。`-` で始まるファイル名には `--` を使用できます。

```sh
./target/release/makefile-fmt -- -Makefile
```

| 終了コード | 意味 |
| --- | --- |
| 0 | 成功。`--diff` は差分があっても 0 |
| 1 | `--check` で差分あり |
| 2 | unsupported feature / unsafe input / CLI 引数エラー |
| 3 | I/O エラー / shfmt の起動失敗・必須機能不足など |

整形時に GNU Make や recipe の command を実行することはありません。shell 構文の整形だけを shfmt に委譲します。recipe がないファイルでも、処理開始時に shfmt の必須機能を検査します。

## 整形範囲

例えば、

```make
CC=gcc
CFLAGS  :=   -O2

all:
	@echo    hello
	@if true; then \
	echo yes; \
	fi
```

は以下になります。

```make
CC = gcc
CFLAGS := -O2

all:
	@echo hello;
	@if true; then \
		echo yes; \
	fi;
```

recipe は logical command ごとに処理し、shell の起動単位と先頭の `@`・`-`・`+` を保持します。shfmt は POSIX dialect、TAB indentation、simplify 無効、EditorConfig 無効、explicit semicolons 有効に固定します。

初期版は以下を原文のまま保持します。

* `define` 本文、未知の構文、conditional 内の整形対象。
* 複雑な rule、inline recipe、およびその rule 配下の recipe。
* `$`、backtick、heredoc、shell comment を含む command。
* 引用符の状態や改行の再構築を安全に判断できない command。
* shfmt が parse できない command。

`#` や `<<` を含む command は、引用符内にある場合も保守的に保持します。改行コードが混在する logical command も保持します。

単純な変数代入の operator は `=`、`:=`、`::=`、`:::=`、`?=`、`+=`、`!=` に対応します。RHS の内容と末尾空白は保持し、空行数や全行の trailing whitespace は変更しません。未変更領域の bytes、CRLF、末尾改行の有無も保持します。

## 安全性の境界

Unix 系環境と GNU Make 4.4.1 の通常の shell 実行モデルを前提にします。外部からの `SHELL` / `.SHELLFLAGS` の変更や、特殊名・`eval` 呼び出しの動的生成は保証対象外です。

次の構文はファイル全体を fatal unsupported とします。

* `.ONESHELL` / `.POSIX` 特殊ターゲット。
* `SHELL` / `.SHELLFLAGS` / `.RECIPEPREFIX` の設定・変更。
* `include` / `-include` / `sinclude` / `load` / `-load`。
* 危険な特殊名そのものを RHS に持つ単純な literal alias。
* 明示的な `$(eval ...)` / `${eval ...}`。これはコメント・recipe・define 本文でも拒否します。

通常の fatal 検査ではコメント・recipe・define 本文を除外し、conditional の両 branch を検査します。`$(OBJS): common.h` のような通常の variable-expanded target は許可します。

formatter の成功は、入力が supported subset の前提を満たすことを証明するものではありません。入力が前提を満たし、formatter が成功した場合に意味保存を保証します。

`-w` は全検査と整形の成功後に、一時ファイルを atomic rename して更新します。unsupported や shfmt の設定エラー・起動失敗時には元のファイルを書き換えません。symlink はリンク先を更新し、権限ビットを保持します。複数の hard link があるファイルの変更は exit 3 で拒否します。所有者・ACL・拡張属性の引き継ぎは保証しません。

## 開発・検証

実行結果の比較テストには PATH 上の **GNU Make 4.4.1** と対応版 shfmt が必要です。テストは Make のバージョンを検査し、別バージョンでの検証を成功扱いにしません。

```sh
make --version
shfmt --version
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

fixture の期待値・冪等性、GNU Make での整形前後の実行結果、assignment / directive 境界、define 本文の保持、CRLF、CLI の終了コードと no-write 保証を検証します。

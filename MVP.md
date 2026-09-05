# makefile-fmt MVP

## 目的

GNU Makefile を対象に、**意味を変えないことを最優先とした保守的な formatter** を作る。

GNU Make の完全な parser や evaluator は実装しない。

Makefile の外側の構造だけを認識し、**安全に変更できると判断できる箇所だけを整形する**。

recipe 内の shell script の整形は独自実装せず、明示的なセミコロンを付与できる fork 版 `shfmt` に委譲する。

設計原則は以下。

> Parse what is necessary, preserve what is not understood.

さらに、

> 安全性を確認できない箇所には変更を加えない。

優先順位は、

1. 意味を変えない
2. 元のコードを壊さない
3. diff を小さくする
4. 見た目を整える

とする。

---

## Supported subset と意味保存の保証範囲

任意の GNU Make 実行環境に対する意味保存までは保証しない。

`makefile-fmt` v0.1 の semantic-preservation の保証基準は **GNU Make 4.4.1** とする。

それ以前の GNU Make、特に 3.x 系との互換性は保証しない。assignment / directive の判定など、バージョンによって構造認識そのものが変わるケースについて、MVP で複数バージョン対応は行わない。

例えば `include=value` / `include = value` は GNU Make 4.4.1 の変数代入として扱う。formatter 自体が GNU Make のバージョンや parser / evaluator を完全再現することは目標にしない。

MVP の supported subset は以下を前提とする。

* Unix 系環境である。
* GNU Make 4.4.1 の挙動を基準とする。
* GNU Make の通常の shell 実行モデルを使用する。
* Makefile 外部から `SHELL` や `.SHELLFLAGS` 等を変更しない。
* Makefile 外部から parsing / recipe semantics を変更しない。
* 特殊ターゲット名・特殊変数名・`eval` 呼び出しを、変数展開、文字列連結、computed name、関数等によって動的に生成しない。

例えば `make SHELL=/bin/bash` のような外部指定や、以下の動的生成は保証対象外とする。

```make
A = .ONE
B = SHELL
MODE = $(A)$(B)
$(MODE):
```

ただし、`$` を含む target 名を一律 fatal にはしない。

```make
$(OBJS): common.h
```

のような通常の variable-expanded target は許可する。整形の安全性を判断できない場合は原文を保持する。

危険な特殊名のリテラルな使用、および後述する単純な literal alias は静的に検出して fatal とする。Make 式を完全評価して特殊名を探索することはしない。

> formatter の成功は、入力が supported subset の前提を満たすことを証明するものではない。

`makefile-fmt` が検出するのは、MVP で静的に認識すると決めた unsupported feature だけである。

以下の名前などが間接的に生成されていないことまでは証明しない。

```text
.ONESHELL
.POSIX
SHELL
.SHELLFLAGS
.RECIPEPREFIX
eval
```

保証モデルは以下とする。

```text
入力が supported subset の前提を満たしている
AND
makefile-fmt が成功した
    ↓
formatting によって意味を変えないことを保証する
```

`makefile-fmt` の成功だけから、入力が supported subset であると結論してはならない。

安全性ルールは以下とする。

```text
局所的に理解できない
    → preserve

global semantics を直接変更する構文
    → fatal unsupported

global semantics を動的に生成する構文
    → supported subset の外
    → makefile-fmt は完全評価して検出しない

安全性を確認できた箇所
    → format
```

明示的な `eval` 呼び出しは、動的生成の入口として別途 fatal にする。

---

## 名前

```text
repository: makefile-fmt
package:    makefile-fmt
binary:     makefile-fmt
crate:      makefile_fmt
```

Rust / Cargo の package と binary 名は kebab-case を使用する。

Rust コード内では `makefile_fmt` として参照する。

---

# MVP の処理フロー

```text
read entire file
    ↓
pre-scan / minimal structural scan
    ↓
fatal unsupported feature check + context-independent eval check
    ↓
shfmt capability check
    ↓
lossless structural scan
    ↓
safe formatting
    ↓
recipe extraction
    ↓
forked shfmt
    ↓
Make recipe として再構築
    ↓
apply edits
    ↓
stdout / check / diff / write
```

重要なのは、ファイルを逐次変更しながら処理しないこと。

最初に全文を読み、静的に検出する unsupported feature がないことと、shfmt の必須機能が利用できることを確認してから edit を生成する。

この検査は supported subset の前提そのものを証明するものではない。

---

# 構文の3分類

## Supported

安全に認識・変更できる構文。

formatter が整形する。

## Opaque

完全には理解しない、または MVP では整形しない構文。

元の bytes をそのまま保持する。

formatter 自体は継続する。

## Unsupported

存在すると formatter が Makefile 全体の semantics や recipe 実行モデルを安全に確定できなくなる構文。

検出した場合は、

```text
何も変更しない
↓
diagnostic を出す
↓
non-zero exit
```

とする。

---

# Fatal Unsupported

MVP では以下を全文 pre-scan で検出した場合、ファイル全体を unsupported とする。

## 検査の context

通常の fatal scan は文字列の単純検索ではなく、最低限の structural scan により構文上の使用を検出する。

```text
comment      → 対象外
recipe       → 対象外
define body  → 対象外
Make syntax  → 対象
conditional  → 両 branch 対象
```

例えば `# .ONESHELL:` は無視する。以下の `SHELL=...` も recipe の一部なので、Make の変数設定としては検出しない。

```make
foo:
	echo SHELL=/bin/bash
```

conditional は評価せず、有効かどうかに関係なく両 branch を検査する。

```make
ifeq ($(X),1)
.ONESHELL:
endif
```

この例は fatal とする。

ただし、明示的な `eval` 呼び出しだけは別の検査を持ち、上記の除外 context に関係なく fatal とする。

---

## `.ONESHELL`

```make
.ONESHELL:
```

recipe の shell invocation 単位が変わるため扱わない。

---

## `.POSIX`

```make
.POSIX:
```

通常の shell フラグや行継続の扱いに影響するため扱わない。

---

## `.RECIPEPREFIX`

```make
.RECIPEPREFIX := >
```

recipe の認識規則自体が変わるため扱わない。

MVP はデフォルトの TAB recipe のみ対象とする。

---

## `SHELL` の設定・変更

例:

```make
SHELL := /bin/bash
```

```make
SHELL = /bin/sh
```

```make
foo: SHELL := /bin/bash
```

global / target-specific を含め、`SHELL` の設定を認識したら unsupported とする。

MVP の recipe formatting は GNU Make のデフォルト shell 設定を前提とする。

---

## `.SHELLFLAGS` の設定・変更

```make
.SHELLFLAGS := -ec
```

shell の起動引数を変更するため扱わない。

`SHELL` / `.SHELLFLAGS` / `.RECIPEPREFIX` は通常 assignment だけでなく、構文上それらを定義・変更するものを対象とする。

```make
override SHELL := /bin/bash
foo: SHELL := /bin/bash
define SHELL
/bin/bash
endef
```

これらも fatal とする。`define` の開始行は変数の定義として検査する。

---

## `include` 系

```make
include foo.mk
-include foo.mk
sinclude foo.mk
```

include 先から `.ONESHELL`、`.RECIPEPREFIX`、`SHELL` 等が導入される可能性がある。

MVP では include graph を追跡しない。

---

## 明示的な `eval` 呼び出し

```make
$(eval ...)
${eval ...}
```

`eval` は、Makefile 構文を動的に導入できる明示的な escape hatch として禁止する。

通常の structural context と独立した検査を行い、明示的な呼び出しは出現 context に関係なく fatal unsupported とする。comment、recipe、define body も例外にしない。

```make
all:
	$(eval SHELL := /bin/bash)
	echo hello
```

```make
define FOO
$(eval .ONESHELL:)
endef
```

どちらも fatal とする。`FOO` が実際に展開されるかどうかは追跡しない。recipe を局所的に skip して、この検査を回避することはできない。

一方、以下のような呼び出し自体の動的生成までは追跡しない。

```make
F = eval
$($(F) ...)
```

これは supported subset の前提外とする。

---

## `load` / `-load`

```make
load extension.so
-load extension.so
```

GNU Make 自体を動的に拡張できるため扱わない。

---

## 危険な特殊名への単純な literal alias

単純な assignment RHS が危険な特殊名そのものの場合も fatal とする。変数の使用先や実際に展開されるかどうかは追跡しない。

```make
X = .ONESHELL
X := .POSIX
X = SHELL
X = .SHELLFLAGS
X = .RECIPEPREFIX
```

したがって、以下も alias の定義時点で fatal になる。

```make
MODE = .ONESHELL
$(MODE):
```

```make
NAME = SHELL
$(NAME) := /bin/bash
```

一方、単に文字列の一部に特殊名を含むだけでは fatal にしない。

```make
HELP = use your SHELL to run commands
```

この alias 検査にも通常の fatal scan の context 規則を適用する。

---

## Fatal Unsupported 一覧

```text
.ONESHELL
.POSIX
.RECIPEPREFIX の設定・変更
SHELL の設定・変更
.SHELLFLAGS の設定・変更
include
-include
sinclude
$(eval ...)
${eval ...}
load
-load
危険な特殊名への単純な literal alias
```

このいずれかを検出した場合、

```text
makefile-fmt: Makefile:42: unsupported GNU Make feature: .ONESHELL
```

のような diagnostic を出し、入力は一切変更しない。

`-w` の場合も書き込みを行わない。

---

# Fatal にしない構文

局所的にその部分を変更しなければ安全なものは、formatter 全体を止めない。

## `define ... endef`

```make
define FOO
...
endef
```

fatal 検査を通過した block 全体を opaque とする。

内部は byte-for-byte で保持する。

```make
define FOO
include foo.mk
.ONESHELL:
endef
```

この本文は `FOO` の値であり、`include` や `.ONESHELL:` を通常の fatal scan の対象にはしない。

ただし、`define SHELL` 等の特殊変数の定義、および context に関係なく検査する明示的な `eval` 呼び出しは fatal とする。

---

## complex rule

例:

```make
foo &: bar
foo:: bar
%.o: %.c
foo: | build
```

MVP で安全に整形できない場合、その rule をそのまま保持する。

---

## inline recipe

```make
foo: ; echo hello
```

Makefile 全体としては unsupported にしない。

ただし shell formatting の対象外。

---

## conditional

```make
ifeq (...)
...
endif
```

structural boundary を認識するが、条件式は評価しない。

条件式や indentation を整形する必要はない。通常の fatal scan は両 branch に適用する。

---

## unknown syntax

scanner が分類できない行は `Raw` / `Unknown` として保存する。

未知であること自体はエラーにしない。

---

# Structural Scanner

完全な AST は作らない。

元の source span を保持する lossless scanner とする。

概念的には、

```rust
enum LineKind {
    Blank,
    Comment,
    Assignment,
    Rule,
    Recipe,
    Directive,
    Conditional,
    DefineStart,
    DefineBody,
    DefineEnd,
    Unknown,
}
```

程度。

各行は元の文字列を保持する。

```rust
struct Line<'a> {
    number: usize,
    raw: &'a str,
    kind: LineKind,
}
```

scanner state は MVP では最小限にする。

```rust
struct ScanState {
    in_define: bool,
    after_rule: bool,
}
```

`.ONESHELL`、`.RECIPEPREFIX`、`SHELL` 等は fatal にしているため、複雑な global state を scanner に持たせない。

---

# Edit ベースの formatter

Node から Makefile を再生成しない。

元 source に対する edit の集合として formatting を表現する。

```rust
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}
```

最後に non-overlapping edits を適用する。

これにより、変更対象外の箇所は完全に原文を維持する。

---

# Assignment Formatting

MVP では安全に認識できる単純な assignment のみ整形する。

対象 operator:

```text
=
:=
::=
:::=
?=
+=
!=
```

lexer は longest-match する。

例:

```make
CC=gcc
CFLAGS  :=   -O2
```

を、

```make
CC = gcc
CFLAGS := -O2
```

にする。

ただし RHS は opaque string として扱う。

RHS の中身や末尾空白は変更しない。

複雑な assignment は untouched とする。

例:

```make
target: CFLAGS := -O0
override CFLAGS += -g
export CC := gcc
```

MVP で安全に扱えない場合は整形しない。

`SHELL` / `.SHELLFLAGS` / `.RECIPEPREFIX` の設定・変更、および危険な特殊名への単純な literal alias は formatting 以前に fatal unsupported。

---

# Trailing Whitespace

MVP では全行一律の trailing whitespace 削除は行わない。

assignment RHS、recipe、continuation 等では trailing whitespace が意味を持つ可能性がある。

したがって、

> trailing whitespace は「安全と証明できる context だけ」で削除する。

という方針にする。

初期版では無理に coverage を広げない。

---

# Recipe Recognition

MVP はデフォルト TAB recipe のみを扱う。

`.RECIPEPREFIX` は unsupported なので、

```make
foo:
	echo hello
```

のような形式だけを recipe として認識する。

space-indented な、

```make
foo:
    echo hello
```

を TAB に修正することはしない。

これは formatting ではなく repair になり得るため、MVP の対象外とする。

---

# Recipe の単位

`.ONESHELL` を禁止しているため、recipe block 全体を一度に shfmt に渡してはいけない。

通常の GNU Make では logical recipe command ごとに shell invocation が分かれる。

例えば、

```make
foo:
	echo one
	echo two
```

は、

```text
command 1: echo one
command 2: echo two
```

として扱う。

内部表現も、

```rust
struct RecipeCommand<'a> {
    lines: Vec<&'a str>,
}
```

のように logical recipe command 単位にする。

`@`、`-`、`+` は logical recipe command 先頭の Make prefix として認識する。shfmt に渡す前に取り外し、整形後に同じ prefix 列を先頭へ復元する。

---

# Forked shfmt

shell formatting は fork 版 `shfmt` に委譲する。

formatter の整形処理を開始する前に capability check を行い、必須の `--explicit-semicolons` 機能が利用できることを確認する。

以下は tool configuration error として fatal、exit 3 とする。

```text
shfmt executable がない
必要な explicit-semicolon 機能がない
subprocess の起動自体に失敗
```

対応版 shfmt が個別 recipe を parse できない場合とは区別する。

整形結果を環境依存にしないため、dialect は POSIX、indentation は TAB、simplify は無効、EditorConfig の影響は無効とし、`makefile-fmt` 側で固定する。`--explicit-semicolons` を必ず有効にする。

fork 側は shell statement boundary を明示的な semicolon として出力する。

例えば shell fragment を、

```sh
if foo; then
	echo yes;
fi;
```

のように出力できることを前提とする。

Rust 側では、安全性を確認できた shell newline のみを Make の logical recipe command を維持する形へ再構築する。

Make の `\` + newline は一律に削除・再挿入しない。元の logical recipe command の境界と shell invocation の意味を維持できる場合だけを整形対象とする。shfmt 出力を安全に再構築できない場合も、元の command を保持する。

概念的には、

```text
shfmt output
    ↓
安全に変換できる shell newline を Make continuation に変換
    ↓
各 physical line に TAB recipe prefix を付与
```

する。

例:

```sh
if foo; then
	echo yes;
fi;
```

を Makefile に戻す際には、

```make
	if foo; then \
		echo yes; \
	fi;
```

のように、一つの logical recipe command を維持する。

---

# Recipe-level Skip

Makefile 全体としては対応可能でも、安全に shfmt できない recipe command はその部分だけ untouched にする。

これは fatal error ではない。

MVP は allow-list 方式とし、安全性を確認できた command だけを整形する。

初期版では少なくとも以下を skip する。

```text
$ を含む
heredoc を含む
shell comment を含む
quote 状態を安全に判定できない
quote 内の backslash-newline を含む
inline recipe
対応版 shfmt が parse できない
scanner が recipe boundary を確定できない
その他 scanner が安全に shell fragment 化・再構築できないもの
```

明示的な `eval` 呼び出しはこの skip 規則より優先し、ファイル全体を fatal unsupported とする。

---

# Make Expansion Masking

最終的には recipe 内の、

```make
$(VAR)
${VAR}
$@
$$x
```

などを扱う。

MVP の初期段階では masking を行わず、以下で固定する。

```text
recipe command に `$` が存在する
    → shfmt skip
```

ただし、明示的な `eval` 呼び出しは事前の検査で fatal とする。

将来 coverage を広げる場合は、

```text
1. $$ support
2. simple $(VAR) / ${VAR}
3. automatic variables
4. complex Make expression
```

の順に検討する。これらは初期 MVP の整形対象には含めない。

---

# Blank Lines

安全な通常領域に限り、連続空行を一定数までに制限できる。

例:

```text
max consecutive blank lines = 2
```

ただし、

```make
define ...
...
endef
```

などの opaque block 内では変更しない。

---

# CLI

MVP の CLI は以下。

```bash
makefile-fmt Makefile
```

整形結果を stdout に出力。

```bash
makefile-fmt -w Makefile
```

上書き。

```bash
makefile-fmt --check Makefile
```

format 差分が存在するか確認。

CI 用途では差分がある場合 non-zero exit。

```bash
makefile-fmt --diff Makefile
```

diff を表示。

---

# Exit Code

以下とする。

```text
0 = success / already formatted
1 = --check で formatting difference あり
2 = unsupported feature / unsafe input
3 = I/O error / tool configuration error
```

対応版 shfmt が特定 recipe を parse できない場合は、その recipe command を untouched にして formatter を継続する。

shfmt executable の不在、必須機能の不足、subprocess の起動失敗は fatal、exit 3 とする。これらの場合も `-w` の書き込みを行わない。

---

# Idempotency

必須条件。

```text
format(format(source)) == format(source)
```

を保証する。

fixture 全件に対して idempotency test を実行する。

---

# 必須テスト

最低限以下を用意する。

実行結果を比較するテストは GNU Make 4.4.1 を使用する。テスト開始時にバージョンを確認し、古い GNU Make の結果を保証基準の検証として扱わない。通常の formatter 実行では GNU Make を起動せず、この確認はテスト環境だけで行う。

```text
simple assignment
GNU Make 4.4.1 の assignment / directive 境界（include=value / include = value 等）
simple rule
simple recipe
multi-line shell recipe
define/endef preservation
unknown syntax preservation

.ONESHELL rejection
.POSIX rejection
.RECIPEPREFIX rejection
SHELL assignment rejection
.SHELLFLAGS assignment rejection
override / target-specific / define による特殊変数の設定の rejection
危険な特殊名への単純な literal alias の rejection
通常の variable-expanded target の許可・原文保持
特殊名を文字列の一部に含む通常 assignment の許可
include rejection
$(eval ...) / ${eval ...} rejection
comment / recipe / define body 内の明示的な eval の rejection
間接的に生成される eval 呼び出しを追跡しない
comment / recipe / define body 内の通常の特殊構文の原文保持
conditional の両 branch の fatal 検査
load rejection

$ / heredoc / shell comment / unsafe quote を含む recipe の原文保持
@ / - / + prefix の保持
shfmt 不在・必須機能不足・起動失敗時の exit 3 / no-write
対応版 shfmt の個別 parse failure 時の command 保持・処理継続
shfmt 設定の固定・EditorConfig の影響の排除

-w 時の unsupported input が byte-for-byte unchanged
opaque region が byte-for-byte unchanged
idempotency
CRLF / final newline preservation
```

特に、

```text
unsupported input
→ output file untouched
```

は強い invariant とする。

---

# MVP の非目標

以下は MVP ではやらない。

```text
GNU Make の完全 parser
複数バージョンの構造認識への対応・GNU Make の完全なバージョン再現
Make expression の意味解析
include graph の解決
eval 引数の意味解析・評価（明示的な呼び出しの検出は行う）
特殊名や eval 呼び出しの動的生成の追跡
SHELL の追跡
.ONESHELL 対応
.POSIX 対応
.RECIPEPREFIX 対応
.SHELLFLAGS の変更への対応
space-indented recipe の修復
Makefile 全体の pretty-print
rule dependency list の再整形
conditional indentation
comment reflow
完全な Make expansion masking
GNU Make 以外の make dialect 対応
```

---

# MVP 完成条件

以下が動けば v0.1 とする。

```text
✓ 全文 pre-scan
✓ fatal unsupported detection
✓ context に依存しない明示的な eval 呼び出しの検出
✓ 危険な特殊名への単純な literal alias の検出
✓ lossless structural scanner
✓ Raw / Opaque preservation
✓ define/endef preservation
✓ simple assignment spacing
✓ rule / TAB recipe recognition
✓ logical recipe command extraction
✓ shfmt capability check / 固定設定
✓ forked shfmt invocation
✓ multiline shell の Make continuation 再構築
✓ unsafe recipe の skip
✓ --check
✓ --diff
✓ -w
✓ idempotency
✓ unsupported 時の no-write guarantee
✓ tool configuration error 時の no-write guarantee
```

MVP の目的は、

> 多くの Makefile を整形できること

ではなく、

> supported subset の前提を満たす Makefile について、formatter が成功した場合に、意味を変えずに確実に整形すること

とする。

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
pre-scan
    ↓
fatal unsupported feature check
    ↓
structural scan
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

最初に全文を読み、安全に扱える Makefile であることを確認してから edit を生成する。

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

## `.ONESHELL`

```make
.ONESHELL:
```

recipe の shell invocation 単位が変わるため扱わない。

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

## `include` 系

```make
include foo.mk
-include foo.mk
sinclude foo.mk
```

include 先から `.ONESHELL`、`.RECIPEPREFIX`、`SHELL` 等が導入される可能性がある。

MVP では include graph を追跡しない。

---

## `$(eval ...)`

```make
$(eval ...)
```

任意の Makefile syntax を動的に生成できるため扱わない。

---

## `load` / `-load`

```make
load extension.so
-load extension.so
```

GNU Make 自体を動的に拡張できるため扱わない。

---

## Fatal Unsupported 一覧

```text
.ONESHELL
.RECIPEPREFIX
SHELL assignment
include
-include
sinclude
$(eval ...)
load
-load
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

block 全体を opaque とする。

内部は byte-for-byte で保持する。

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

structural boundary の認識だけ行ってもよい。

内容を理解できない場合はそのまま保持する。

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

`SHELL` assignment は formatting 以前に fatal unsupported。

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

---

# Forked shfmt

shell formatting は fork 版 `shfmt` に委譲する。

fork 側は shell statement boundary を明示的な semicolon として出力する。

例えば shell fragment を、

```sh
if foo; then
	echo yes;
fi;
```

のように出力できることを前提とする。

Rust 側では、その shell newline を Make の logical recipe command を維持する形へ再構築する。

概念的には、

```text
shfmt output
    ↓
shell newline を Make continuation に変換
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

MVP では必要に応じて以下を skip する。

```text
inline recipe
Make expansion の masking が安全でない
forked shfmt が parse できない
scanner が recipe boundary を確定できない
```

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

ただし MVP の初期段階では、無理に完全対応しなくてよい。

最も保守的には、

```text
recipe command に `$` が存在する
    → shfmt skip
```

から始めてもよい。

その後、

```text
1. $$ support
2. simple $(VAR) / ${VAR}
3. automatic variables
4. complex Make expression
```

の順に coverage を広げる。

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

例として、

```text
0 = success / already formatted
1 = --check で formatting difference あり
2 = unsupported feature / unsafe input
3 = I/O error / external formatter unavailable
```

とする。

forked shfmt が特定 recipe を parse できないだけなら、その recipe を untouched にして成功扱いでもよい。

shfmt executable 自体を起動できない場合は fatal とする。

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

```text
simple assignment
simple rule
simple recipe
multi-line shell recipe
define/endef preservation
unknown syntax preservation

.ONESHELL rejection
.RECIPEPREFIX rejection
SHELL assignment rejection
include rejection
eval rejection
load rejection

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
Make expression の意味解析
include graph の解決
$(eval ...) の解析
SHELL の追跡
.ONESHELL 対応
.RECIPEPREFIX 対応
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
✓ lossless structural scanner
✓ Raw / Opaque preservation
✓ define/endef preservation
✓ simple assignment spacing
✓ rule / TAB recipe recognition
✓ logical recipe command extraction
✓ forked shfmt invocation
✓ multiline shell の Make continuation 再構築
✓ unsafe recipe の skip
✓ --check
✓ --diff
✓ -w
✓ idempotency
✓ unsupported 時の no-write guarantee
```

MVP の目的は、

> 多くの Makefile を整形できること

ではなく、

> 対応すると宣言した Makefileについて、意味を変えずに確実に整形すること

とする。

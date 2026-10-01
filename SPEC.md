# makefile-fmt Specification

この文書は `makefile-fmt` の正規仕様である。README は導入と使い方、CHANGELOG はリリース履歴を扱う。

v0.4.0 の実装が保証する対象範囲、安全性の境界、CLI の挙動を定義する。

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

`makefile-fmt` の semantic-preservation の保証基準は **GNU Make 4.4.1** とする。

それ以前の GNU Make、特に 3.x 系との互換性は保証しない。assignment / directive の判定など、バージョンによって構造認識そのものが変わるケースについて、makefile-fmt で複数バージョン対応は行わない。

例えば `include=value` / `include = value` は GNU Make 4.4.1 の変数代入として扱う。formatter 自体が GNU Make のバージョンや parser / evaluator を完全再現することは目標にしない。

makefile-fmt の supported subset は以下を前提とする。

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

`makefile-fmt` が検出するのは、makefile-fmt で静的に認識すると決めた unsupported feature だけである。

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

# 処理フロー

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

完全には理解しない、または makefile-fmt では整形しない構文。

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

makefile-fmt では以下を全文 pre-scan で検出した場合、ファイル全体を unsupported とする。

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

makefile-fmt はデフォルトの TAB recipe のみ対象とする。

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

makefile-fmt の recipe formatting は GNU Make のデフォルト shell 設定を前提とする。

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

makefile-fmt では include graph を追跡しない。

---

## 明示的な `eval` 呼び出し

```make
$(eval ...)
${eval ...}
```

`eval` は、Makefile 構文を動的に導入できる明示的な escape hatch として禁止する。

通常の structural context と独立した検査を行い、明示的な呼び出しは出現 context に関係なく fatal unsupported とする。comment、recipe、define body も例外にしない。

実装の検出範囲は、実際の Make `eval` 呼び出しより保守的である。入力 bytes 中の `$(eval ...)` / `${eval ...}` という呼び出し形を検査し、この検査では `$$` を字句解析しない。そのため `$$(eval echo hi)` や `$${eval ...}` も内部の呼び出し形に一致し、exit 2 の fatal unsupported となる。`$$(eval echo hi)` は Make の `eval` 呼び出しではなく shell 側の command substitution を表せるが、それも意図的に拒否する範囲に含める。単に `eval` という文字列が含まれるだけで一律に拒否するわけではない。この保守的な挙動は現行版にも引き継がれる。

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

makefile-fmt で安全に整形できない場合、その rule をそのまま保持する。

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

scanner state は makefile-fmt では最小限にする。

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

makefile-fmt では安全に認識できる単純な assignment のみ整形する。

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

makefile-fmt で安全に扱えない場合は整形しない。

`SHELL` / `.SHELLFLAGS` / `.RECIPEPREFIX` の設定・変更、および危険な特殊名への単純な literal alias は formatting 以前に fatal unsupported。

---

# Trailing Whitespace

makefile-fmt では全行一律の trailing whitespace 削除は行わない。

assignment RHS、recipe、continuation 等では trailing whitespace が意味を持つ可能性がある。

したがって、

> trailing whitespace は「安全と証明できる context だけ」で削除する。

という方針にする。

安全性を確認できない構文へ整形範囲を広げない。

---

# Recipe Recognition

makefile-fmt はデフォルト TAB recipe のみを扱う。

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

これは formatting ではなく repair になり得るため、makefile-fmt の対象外とする。

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

makefile-fmt は allow-list 方式とし、安全性を確認できた command だけを整形する。

以下を含む recipe command は、現行の safety check と masking で安全に
処理できる場合を除き skip する。

```text
未対応または安全に mask できない `$`
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


## 目的

recipe 内に Make 変数展開があっても、shell 部分だけを安全に `shfmt` へ渡せるようにする。

代表例:

```make
foo.o:
	$(CC) $(CFLAGS) -c $< -o $@
```

を shfmt 対象にできるようにする。

## 基本方針

Make expansion は評価しない。

recipe を、

```text
Make syntax
+
shell syntax
```

として扱い、Make 側の要素を一時的に mask して shell skeleton だけを `shfmt` に渡す。

```text
recipe
→ Make `$` lexer
→ Make expansion を placeholder 化
→ `$$` を shell `$` に変換
→ shfmt
→ placeholder round-trip と shell `$` の由来を検証
→ 元の `$$` に由来する shell `$` だけを `$$` に戻す
→ placeholder 復元
```

## 対応対象

まず以下を扱う。

```make
$(VAR)
${VAR}

$@
$<
$^
$?
$*
$%

$$
```

`$(CC)`、`$(CFLAGS)`、automatic variables を主要ユースケースとする。

`$` は左から字句解析する。`$$` は1組として shell `$` に対応し、`$$$$` は `$$` × 2 とする。`$$$` は `$$` + 未対応の末尾 `$` となるため command 全体を skip する。

対応構文は明示的な allow-list とし、`$|`、`$0`、`$x`、単独の `$`、malformed expression 等は command 単位で skip する。nested expression の内部は評価せず、一つの opaque expression として保持する。

## Placeholder

例えば、

```make
$(CC) $(CFLAGS) -c $< -o $@
```

を内部的に、

```sh
__MAKEFMT_0__ __MAKEFMT_1__ -c __MAKEFMT_2__ -o __MAKEFMT_3__
```

のようにして `shfmt` へ渡す。

shfmt 後に元の Make expression を完全に復元する。

placeholder は shell の通常の word として扱える形式とし、元入力および既存 placeholder と衝突しない値を選ぶ。single quote / double quote 内の Make expression も masking 対象とする。

shfmt 後、各 placeholder がちょうど1回、完全な形で残っていることを検証する。欠落・重複・変形があれば command 全体を untouched にする。

## `$$`

```make
echo "$$HOME"
```

は shell が実際に受け取る形、

```sh
echo "$HOME"
```

として shfmt に渡す。

`$$` 由来の shell `$` は専用 placeholder 等で追跡する。復元時には、元の `$$` に由来するものだけを Make recipe 用の `$$` に戻す。shfmt 出力中の他の `$` を一律変換したり、復元済みの Make expression 内の `$` を変換したりしない。

## Parser

regex だけで処理せず、最低限の Make-dollar lexer を作る。

特に、

```make
$(...)
${...}
```

は対応する括弧まで正しく読み取る。

nested expression も「中身を解釈せず、一つの opaque Make expression」として扱えるようにする。

## Safety model

Make expansion の値そのものは評価しない。保証条件は、当初の「単一の word または word fragment」という制約から、以下の **syntactic-role preservation（構文上の役割の維持）** に置き換える。

* Make expansion は、通常の引数列や word fragment を生成してよい。
* placeholder で認識した shell 構造に対し、Make expansion 前後で command、reserved word、operator、redirection、quote、control structure 等の構文上の役割を変えてはならない。
* 引数位置での空展開は、その shell command の成立と構文上の役割を維持する場合に限り許可する。
* command position の expansion、または shell list 内の独立した command に相当する expansion が空になり、その command 自体が消滅するケースは保証対象外とする。

通常の argument word 数が変わること自体は禁止しない。placeholder を含む AST と Make 展開後の AST が word 数まで完全に一致することを要求するのではなく、構文上の役割が維持されることを要求する。

例えば、次は対象となる。

```make
CFLAGS = -O2 -Wall
foo.o: foo.c
	$(CC) $(CFLAGS) -c $<
```

`CFLAGS =` のような空の引数列も、`$(CC)` が有効な command 名を生成し、command が成立する場合には対象となる。

一方、expansion が `;`、`&& echo done`、`>out`、未閉鎖の quote 等を生成する場合や、command position で reserved word を導入する場合など、構文上の役割を変えるケースは保証対象外とする。

### Command disappearance と no-op

空展開に関する制約は recipe 全体だけでなく、compound command / shell list 内の個々の command にも適用する。

```make
OPTIONAL =
all:
	(echo ok; $(OPTIONAL))
```

この例は保証対象外である。元は `(echo ok; )` として成立するが、formatter が `$(OPTIONAL)` に相当する command の末尾に `;` を付けると、Make 展開後は `;` だけの空 command が残り syntax error になり得る。

何もしない branch では、空文字列ではなく有効な no-op command を生成する。

```make
	$(if $(X),echo ok,:)
	$(if $(X),echo ok,: nothing to do)
	$(if $(CMD),$(CMD),: nothing to do)
```

`: nothing to do` は command `:` と通常の引数列であり、今回の制約では許可される。`$(CMD)` の非空側についても、構文上の役割を維持する前提は引き続き適用する。

formatter はこれらの値や条件を評価・検出しない。入力側が満たすべき supported subset の前提であり、違反を静的な fatal unsupported / skip として検出する仕様ではない。formatter が成功しても、この前提を満たすことは証明されない。masking、fatal / skip 判定、terminal semicolon の保持は変更しない。

## Skip

以下は安全に処理できなければ recipe command 単位で untouched とする。

* malformed `$(...)` / `${...}`
* Make expansion の境界を確定できない
* placeholder round-trip を保証できない
* shfmt parse failure
* その他既存の recipe skip 条件

heredoc、shell comment、backtick、unsafe quote、inline recipe 等の既存 skip 規則を維持する。

明示的な `$(eval ...)` / `${eval ...}` は masking より前に検出し、出現 context に関係なく file-level fatal とする。

この検査は実際の Make 呼び出しより保守的な、入力 bytes 中の呼び出し形の検出である。`$$` を1組とする Make-dollar lexing より前に行い、`$$(eval echo hi)` や `$${eval ...}` に含まれる呼び出し形も fatal（exit 2）とする。前者が Make の `eval` ではなく shell の command substitution を表す場合も例外にしない。単なる `eval` という文字列の出現すべてを拒否するものではない。既存の fatal scan の挙動を維持し、escaped dollar による除外は追加しない。

優先順位は以下とする。

```text
fatal check
→ existing recipe safety checks
→ Make-dollar lexing
→ masking
→ shfmt
→ placeholder round-trip validation
→ restore
```

## 必須条件

* 元の Make expression を byte-for-byte で復元する
* logical recipe command の境界を変えない
* `@`, `-`, `+` prefix を維持する
* idempotent
* placeholder が元入力と衝突しない
* `$` の Make / shell 境界を壊さない

## 検証要件

Make expression を通常の word 形式の placeholder にし、`$$` を実際の shell `$` に変換した入力を shfmt に渡す。

`$$` がある場合は、その `$` も個別の placeholder にした由来確認用の入力を別途整形する。全 placeholder がちょうど1回残ることを確認し、由来確認用出力の dollar placeholder だけを `$` に戻した結果が実際の shell 整形結果と byte-for-byte で一致することを要求する。その一致を確認した後で、記録した元の Make expression と `$$` を復元する。

由来確認用の入力が parse できない場合や出力が一致しない場合は command 全体を保持する。例えば `$$(command)` は skip する。複数行の Make expression も、改行や TAB を含む元の bytes を確実に復元するため、command 単位で保持する。

Make continuation として再構築した結果を再度同じ処理に通し、復元済みの整形結果が一致することも確認する。

## 対象例

```make
	$(CC) $(CFLAGS) -c $< -o $@
	echo "$$HOME"
	echo "$(NAME)"
	cp $(SRC) $(DST)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@
```

目的は Make expression を理解することではなく、**Make expansion を避けながら shell 部分に shfmt を適用すること**とする。

---

# Rule Header and Recipe Ownership


## 目的

複雑な rule header 配下でも、recipe の所属を安全に判定できる場合は recipe formatting を行えるようにする。

代表例:

```make
$(OUTDIR)/%: %.c | $(OUTDIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDFLAGS) $(LDLIBS)
```

現行実装は header と recipe の安全判定を分離する。構造を安全に認識できる複雑な header 配下では recipe を整形し、header 自体は変更しない。

## 基本方針

rule header 自体は整形しない。

header 内の Make expansion は target / prerequisite の名前やリストを生成する用途を supported subset とする。rule / assignment の区別、inline recipe の有無、recipe の所属、その他 header の構造を動的に生成・変更するケースは supported subset 外とし、formatter はそれを評価・検出しない。formatter の成功は、この前提を満たすことの証明ではない。

```text
rule header
  → opaque / unchanged

recipe ownership
  → 安全に判定できるか確認

recipe
  → recipe-level safety check
  → safe なら masking → shfmt
```

## 対応する rule header

単一の構文上の `:` を持つ通常 rule / pattern rule を対象とする。

少なくとも以下を含んでいても、rule と recipe の境界を確定できる場合は recipe formatting を許可する。

```text
$(VAR) を含む target
% pattern rule
| order-only prerequisite
複雑な prerequisite
上記の組み合わせ
```

例:

```make
$(OUTDIR):
	mkdir -p $(OUTDIR)

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

$(OUTDIR)/%: %.c | $(OUTDIR)
	$(CC) $(CFLAGS) $< -o $@
```

## Skip

以下では recipe formatting を行わない。

* rule と assignment の区別を安全に確定できない
* inline recipe
* rule header continuation の境界を安全に判定できない
* recipe ownership が曖昧
* recipe-level skip 条件に該当する

`::`、`&:`、`&::`、static pattern rule、inline recipe、target-specific assignment は配下の recipe formatting を skip する。conditional / define 等の保持規則も適用する。

## Rule header の構造認識

単なる文字検索ではなく、Make expression、escape、comment の外側にある構文上の `:` を rule delimiter として認識する。`$(...)` / `${...}` 内にある `:`、`;`、`=` 等は header delimiter として扱わない。

expression の未閉鎖などで構造を確定できない場合は Unknown / ambiguous とし、その配下と思われる recipe も変更しない。

```text
rule と安全に判定できる
→ header unchanged
→ recipe ownership を認める

判定不能
→ header unchanged
→ recipe untouched
```

## Recipe formatting の保証

recipe formatting には以下の仕様を適用する。

* Make-dollar lexer
* placeholder masking / restore
* `$$` の由来追跡
* round-trip validation
* fatal unsupported 判定
* recipe-level safety check
* idempotency
* unknown / opaque 部分の保持

## 非目標

以下は行わない。

* rule header の整形
* prerequisite list の整形
* Make expression の評価
* supported subset の大幅拡張
* parser の全面再設計

## 受け入れ条件

```text
✓ 複雑な rule header 配下でも recipe ownership を安全に認識できる
✓ header 自体は byte-for-byte で保持
✓ recipe には recipe-level safety check と masking を適用
✓ ambiguous な場合は skip
✓ idempotent
✓ corpus で semantic issue なし
```

`$(VAR)` を含む通常 rule、`%` pattern rule、`|` order-only prerequisite を含む rule、およびその組み合わせについて、以下を regression test / 検証で確認する。

* header が byte-for-byte 不変
* recipe が実際に formatting される
* idempotent
* GNU Make 4.4.1 で整形前後の実行結果が一致

## 保守的な構造判定

既存の fatal scan と、recipe formatting を許可するための厳格な header 構造検査を分離する。後者では未閉鎖・曖昧な nested expression を拒否し、式と comment の外側に単一の `:` があることを要求する。

expression 外の escape、および `=`・`&` を含む header は comment 内を除き保守的に skip する。escaped filename や `=` を含む literal prerequisite の対応は必須範囲に含めない。header continuation は既存の logical line 処理で折り畳んだ構造が検査を通る場合に認め、元の physical header は変更しない。末尾で途切れた continuation は整形の許可に使わない。

---

# Blank Lines

ファイル先頭の空行は削除し、通常領域で連続する空行は1行に圧縮する。非空入力の末尾は LF 改行1つに統一する。空白行だけの非空入力も LF 改行1つになる。整形対象 recipe の内部レイアウトは shfmt に従う。

ただし、

```make
define ...
...
endef
```

などの opaque block 内では、空行が値の一部なので変更しない。

---

# CLI

makefile-fmt は入力 path をちょうど1件受け取る。入力を省略した場合、複数指定した場合、未知の option を指定した場合は CLI 引数エラー（exit 2）とする。path の解決後に通常ファイルでない場合は I/O error（exit 3）とする。

デフォルトでは整形結果を stdout に出力する。

```bash
makefile-fmt Makefile
```

```bash
makefile-fmt -w Makefile
```

整形結果で入力を更新する。

```bash
makefile-fmt --check Makefile
```

format 差分が存在するか確認。

CI 用途では差分がある場合 non-zero exit。

```bash
makefile-fmt --diff Makefile
```

diff を表示。差分がある場合は終了コード 1、ない場合は 0 を返す。

`-w`、`--check`、`--diff` は同時に指定できない。`--` は option parsing を終了し、`-` で始まる入力ファイル名を指定するために使用する。

`--help` は usage を、`--version` はバージョンを stdout に出力して成功終了する。いずれも入力ファイルおよび `shfmt` を必要としない。

## `-w` の安全性

`-w` は全検査と整形が成功した後、入力ファイルと同じ directory に一時ファイルを作成し、atomic rename で置き換える。走査・整形中に入力ファイルを開いて書き込むことはない。

unsupported input、`shfmt` の設定エラー・起動失敗、または入力が処理中に変更された場合は、入力ファイルを更新しない。symlink を指定した場合はリンク先を更新し、permission bits を保持する。複数の hard link を持つファイルの変更は exit 3 で拒否する。所有者、ACL、拡張属性の引き継ぎは保証しない。

---

# Exit Code

以下とする。

```text
0 = success / already formatted
1 = --check / --diff で formatting difference あり
2 = unsupported feature / unsafe input / CLI 引数エラー
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
CRLF の未変更領域の保持 / final LF newline normalization
```

特に、

```text
unsupported input
→ output file untouched
```

は強い invariant とする。

---

# 非目標

以下は makefile-fmt ではやらない。

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

# 現行の受け入れ条件

以下を満たす。

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
✓ Make expansion の allow-list masking、`$$` の由来追跡、placeholder round-trip 検証
✓ 安全に認識した variable-expanded / pattern / order-only rule 配下の recipe formatting
✓ header と opaque region の byte-for-byte 保持
✓ 先頭空行の削除と通常領域の連続空行の圧縮（define 本文を除く）
✓ 非空入力の末尾 LF 改行1つへの正規化
✓ --check
✓ --diff
✓ -w
✓ idempotency
✓ unsupported 時の no-write guarantee
✓ tool configuration error 時の no-write guarantee
```

makefile-fmt の目的は、

> 多くの Makefile を整形できること

ではなく、

> supported subset の前提を満たす Makefile について、formatter が成功した場合に、意味を変えずに確実に整形すること

とする。

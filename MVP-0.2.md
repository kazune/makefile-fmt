# v0.2 要件

GNU Make 4.4.1 を保証基準とする。v0.1 の保証前提と fatal / skip 規則を引き継ぎ、recipe の `$` による一律 skip のみを以下の allow-list に置き換える。

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

Make expansion の値そのものは評価しない。

supported subset では、

> recipe 内の Make expansion は shell grammar や複数 word、operator を動的生成せず、単一の word または word fragment として展開される

ことを前提とする。

例えば、

```make
$(CC) $(CFLAGS)
```

は対象。

一方、

```make
CMD = if foo; then ...
	$(CMD)
```

のように Make expansion 自体が shell 構文を生成するケースの安全性までは保証しない。

formatter の成功は、この前提を満たしていることの証明ではない。

Make expansion が shell grammar、複数 word、operator 等を生成するケースは supported subset 外とし、評価・検出しない。例えば `$(CFLAGS)` についても、この前提を満たす値を保証対象とする。

## Skip

以下は安全に処理できなければ recipe command 単位で untouched とする。

* malformed `$(...)` / `${...}`
* Make expansion の境界を確定できない
* placeholder round-trip を保証できない
* shfmt parse failure
* その他既存の recipe skip 条件

heredoc、shell comment、backtick、unsafe quote、inline recipe 等の既存 skip 規則を維持する。

明示的な `$(eval ...)` / `${eval ...}` は masking より前に検出し、出現 context に関係なく従来どおり file-level fatal とする。

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

## 最初に通したい例

```make
	$(CC) $(CFLAGS) -c $< -o $@
	echo "$$HOME"
	echo "$(NAME)"
	cp $(SRC) $(DST)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@
```

v0.2 の目的は、Make expression を理解することではなく、**Make expansion を避けながら shell 部分の shfmt 適用範囲を広げること**とする。

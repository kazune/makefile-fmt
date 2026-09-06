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

さらに、Make expansion が command 全体を空にするケースは supported subset 外とする。formatter は Make expansion の値を評価しないため、例えば command position の `$(CMD)` が空に展開されると、整形後に内部 placeholder が消えて `;` だけの shell fragment になり、shell の syntax error を起こすことがある。この条件は formatter の成功だけでは検出・証明しない。

command 全体を Make expansion で生成する場合は、空文字列ではなく有効な no-op command に展開されるようにする。例えば `$(if ...)` を使う場合は、空側で `:` を生成する。

```make
	$(if $(CMD),$(CMD),:)
```

この例では `$(CMD)` が空なら recipe は `:` に展開されるため、formatter が末尾に `;` を付けても有効な shell command のままになる。単に command position の expansion が空になる書き方や、空展開の後ろに `;` だけを残す書き方は避ける。`$(CMD)` が shell grammar、複数 word、operator を生成しないという既存の前提も引き続き適用する。

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

## 初期実装の検証方法

Make expression を通常の word 形式の placeholder にし、`$$` を実際の shell `$` に変換した入力を shfmt に渡す。

`$$` がある場合は、その `$` も個別の placeholder にした由来確認用の入力を別途整形する。全 placeholder がちょうど1回残ることを確認し、由来確認用出力の dollar placeholder だけを `$` に戻した結果が実際の shell 整形結果と byte-for-byte で一致することを要求する。その一致を確認した後で、記録した元の Make expression と `$$` を復元する。

由来確認用の入力が parse できない場合や出力が一致しない場合は command 全体を保持する。例えば `$$(command)` はこの初期実装では skip する。複数行の Make expression も、改行や TAB を含む元の bytes を確実に復元するため、初期実装では command 単位で保持する。

Make continuation として再構築した結果を再度同じ処理に通し、復元済みの整形結果が一致することも確認する。

## 最初に通したい例

```make
	$(CC) $(CFLAGS) -c $< -o $@
	echo "$$HOME"
	echo "$(NAME)"
	cp $(SRC) $(DST)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@
```

v0.2 の目的は、Make expression を理解することではなく、**Make expansion を避けながら shell 部分の shfmt 適用範囲を広げること**とする。

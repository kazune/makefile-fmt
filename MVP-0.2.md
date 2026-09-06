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

明示的な `$(eval ...)` / `${eval ...}` は masking より前に検出し、出現 context に関係なく従来どおり file-level fatal とする。

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

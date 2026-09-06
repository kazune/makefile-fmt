# v0.3 要件

GNU Make 4.4.1 を保証基準とし、v0.2 の保証前提を引き継ぐ。

## 目的

複雑な rule header 配下でも、recipe の所属を安全に判定できる場合は recipe formatting を行えるようにする。

代表例:

```make
$(OUTDIR)/%: %.c | $(OUTDIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDFLAGS) $(LDLIBS)
```

現在は rule header が複雑なため recipe まで skip しているが、v0.3 では header と recipe の安全判定を分離する。

## 基本方針

rule header 自体は整形しない。

header 内の Make expansion は target / prerequisite の名前やリストを生成する用途を supported subset とする。rule / assignment の区別、inline recipe の有無、recipe の所属、その他 header の構造を動的に生成・変更するケースは supported subset 外とし、formatter はそれを評価・検出しない。formatter の成功は、この前提を満たすことの証明ではない。

```text
rule header
  → opaque / unchanged

recipe ownership
  → 安全に判定できるか確認

recipe
  → v0.2 の既存 safety check
  → safe なら masking → shfmt
```

## 対応したい rule header

v0.3 の初期対応は、単一の構文上の `:` を持つ通常 rule / pattern rule に限定する。

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

以下は従来どおり recipe formatting を行わない。

* rule と assignment の区別を安全に確定できない
* inline recipe
* rule header continuation の境界を安全に判定できない
* recipe ownership が曖昧
* v0.2 の既存 recipe skip 条件に該当する

`::`、`&:`、`&::`、static pattern rule、inline recipe、target-specific assignment は引き続き配下の recipe formatting を skip する。conditional / define 等の既存保持規則は変更しない。

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

## 維持するもの

v0.2 の以下の仕様は変更しない。

* Make-dollar lexer
* placeholder masking / restore
* `$$` の由来追跡
* round-trip validation
* fatal unsupported 判定
* recipe-level safety check
* idempotency
* unknown / opaque 部分の保持

## 非目標

v0.3 では以下は行わない。

* rule header の整形
* prerequisite list の整形
* Make expression の評価
* supported subset の大幅拡張
* parser の全面再設計

## 完成条件

```text
✓ 複雑な rule header 配下でも recipe ownership を安全に認識できる
✓ header 自体は byte-for-byte で保持
✓ recipe は v0.2 の既存 formatter を再利用
✓ ambiguous な場合は skip
✓ idempotent
✓ corpus で semantic issue なし
```

`$(VAR)` を含む通常 rule、`%` pattern rule、`|` order-only prerequisite を含む rule、およびその組み合わせについて、以下を regression test / 検証で確認する。

* header が byte-for-byte 不変
* recipe が実際に formatting される
* idempotent
* GNU Make 4.4.1 で整形前後の実行結果が一致

# Release notes

## v0.2.0

### Make expansion masking

* Lex recipe dollars from left to right, supporting `$(...)`, `${...}`,
  `$@`, `$<`, `$^`, `$?`, `$*`, `$%`, and `$$`.
* Mask Make expressions, including nested expressions and quoted occurrences,
  without evaluating their values. Use collision-free placeholders and validate
  that each survives formatting exactly once before restoring the original bytes.
* Track shell dollars originating from `$$` separately; do not indiscriminately
  double dollars in shfmt output.
* Preserve commands containing unsupported or malformed dollar syntax, or whose
  placeholders cannot safely round-trip. Retain the existing fatal and skip rules.

### Known limitations

Recipe formatting remains conservative: recipes under complex or opaque rule
headers may be left unchanged even when their Make expansions are otherwise
supported.

For example, recipes under `$(OUTDIR):` or
`$(OUTDIR)/%: %.c | $(OUTDIR)` remain unchanged. Separating rule-header opacity
from recipe-formatting safety is deferred to a future version.

The semantic-preservation baseline remains GNU Make 4.4.1 and the supported
subset in [MVP.md](MVP.md) and [MVP-0.2.md](MVP-0.2.md). Make expansions must
produce a single shell word or word fragment, not shell grammar, multiple words,
or operators; the formatter does not evaluate or verify those values.

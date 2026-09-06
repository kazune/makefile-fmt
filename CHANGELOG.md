# Release notes

## Unreleased

* Replace the recipe expansion single-word precondition with syntactic-role
  preservation. Ordinary argument lists, word fragments, and empty expansions
  in argument positions that preserve command validity and syntactic roles
  are supported.
* Exclude expansion-driven disappearance of individual commands, including
  elements of compound commands and shell lists, not just entire recipes.
  Use a valid no-op such as `: nothing to do` for an inactive branch.
* These are input-side assumptions, not new fatal or skip checks. Expansion
  values are not evaluated; formatter implementation and semicolon policy
  remain unchanged. Earlier release notes below describe their original scope.

## v0.3.0

* Separate rule-header preservation from recipe-formatting eligibility. Recipes
  under structurally recognized single-colon rules can now be formatted with
  variable-expanded targets/prerequisites, `%` patterns, and `|` prerequisites.
* Keep headers byte-for-byte unchanged, including supported continuations.
  Validate expression boundaries before granting recipe-formatting permission.
* Retain the v0.2 recipe masking, fatal checks, and recipe-level skip rules.
  Double-colon, grouped, static pattern, inline, and target-specific assignment
  headers still do not enable recipe formatting. Ambiguous or escaped headers
  remain conservative.
* Header expansions must only generate names or lists, not change Make header
  structure. Values are not evaluated. GNU Make 4.4.1 remains the baseline.

Release validation reported by the maintainer on an 85-file corpus:

* Dollar-containing recipes formatted: 72 → 95; total recipes formatted: 120 → 146.
* No header changes, ownership errors, non-idempotence, semantic issues, or
  GNU Make 4.4.1 execution differences were observed.
* Fatal unsupported behavior was unchanged from v0.2.

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

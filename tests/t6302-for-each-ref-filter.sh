#!/bin/sh
# Ported subset from git/t/t6302-for-each-ref-filter.sh.

test_description='for-each-ref filtering options'

. ./test-lib.sh

setup_graph () {
	git init repo &&
	cd repo &&

	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&
	A=$(git commit-tree "$EMPTY_TREE" -m one) &&
	B=$(git commit-tree "$EMPTY_TREE" -p "$A" -m two) &&
	C=$(git commit-tree "$EMPTY_TREE" -p "$B" -m three) &&
	D=$(git commit-tree "$EMPTY_TREE" -p "$B" -m four) &&

	git update-ref refs/heads/main "$C" &&
	git update-ref refs/heads/side "$D" &&
	git update-ref refs/odd/spot "$C" &&
	git update-ref refs/tags/one "$A" &&
	git update-ref refs/tags/two "$B" &&
	git update-ref refs/tags/three "$C" &&
	git update-ref refs/tags/four "$D" &&
	git symbolic-ref HEAD refs/heads/main
}

test_expect_success 'setup history and refs' '
	setup_graph
'

# ── --points-at ──────────────────────────────────────────────────────────────

test_expect_success 'filtering with --points-at' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --points-at=main >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with side branch' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --points-at=side >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with tag name resolves to commit' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --points-at=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with root commit' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --points-at=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with two tag' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --points-at=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at combined with pattern' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --points-at=main refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at combined with pattern for tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --points-at=main refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with non-matching pattern returns empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" --points-at=main refs/nonexistent >actual &&
	test_must_be_empty actual
'

# ── --merged ─────────────────────────────────────────────────────────────────

test_expect_success 'filtering with --merged' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=main >actual &&
	test_cmp expect actual
'

test_expect_success '--merged with side branch' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=side >actual &&
	test_cmp expect actual
'

test_expect_success '--merged with root commit only shows root' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --merged=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--merged combined with pattern for heads' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --merged=main refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--merged combined with pattern for tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=main refs/tags >actual &&
	test_cmp expect actual
'

# ── --no-merged ──────────────────────────────────────────────────────────────

test_expect_success 'filtering with --no-merged' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --no-merged=main >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged with side shows what side does not contain' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --no-merged=side >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged with root shows everything except root' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --no-merged=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged combined with pattern for heads' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" --no-merged=main refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged combined with pattern for tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --no-merged=main refs/tags >actual &&
	test_cmp expect actual
'

# ── --contains ───────────────────────────────────────────────────────────────

test_expect_success 'filtering with --contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--contains root commit shows all refs' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--contains leaf commit shows only that ref' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success '--contains with side leaf' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--contains combined with pattern for heads' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--contains combined with pattern for tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two refs/tags >actual &&
	test_cmp expect actual
'

# ── --no-contains ────────────────────────────────────────────────────────────

test_expect_success 'filtering with --no-contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --no-contains=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--no-contains with leaf shows most refs' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --no-contains=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success '--no-contains root commit shows nothing' '
	cd repo &&
	git for-each-ref --format="%(refname)" --no-contains=refs/tags/one >actual &&
	test_must_be_empty actual
'

test_expect_success '--no-contains combined with pattern' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --no-contains=refs/tags/two refs/tags >actual &&
	test_cmp expect actual
'

# ── --contains + --no-contains combined ──────────────────────────────────────

test_expect_success 'filtering with --contains and --no-contains' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --no-contains=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success '--contains + --no-contains with different commits' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --no-contains=refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--contains + --no-contains with same commit gives empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --no-contains=refs/tags/two >actual &&
	test_must_be_empty actual
'

test_expect_success '--contains + --no-contains combined with pattern' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --no-contains=refs/tags/three refs/tags >actual &&
	test_cmp expect actual
'

# ── --merged + --no-merged edge cases ────────────────────────────────────────

test_expect_success '--merged with tag two shows ancestors' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged with tag two' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --no-merged=refs/tags/two >actual &&
	test_cmp expect actual
'

# ── filters combined with --sort ─────────────────────────────────────────────

test_expect_success '--merged combined with --sort=-refname' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/one
refs/odd/spot
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --merged=main --sort=-refname >actual &&
	test_cmp expect actual
'

test_expect_success '--contains combined with --sort=-refname' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/four
refs/odd/spot
refs/heads/side
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --sort=-refname >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged combined with --sort=-refname' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" --no-merged=main --sort=-refname >actual &&
	test_cmp expect actual
'

# ── filters combined with --count ────────────────────────────────────────────

test_expect_success '--merged combined with --count' '
	cd repo &&
	git for-each-ref --format="%(refname)" --merged=main --count=2 >actual &&
	test_line_count = 2 actual
'

test_expect_success '--contains combined with --count' '
	cd repo &&
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --count=3 >actual &&
	test_line_count = 3 actual
'

test_expect_success '--points-at combined with --count' '
	cd repo &&
	git for-each-ref --format="%(refname)" --points-at=main --count=1 >actual &&
	test_line_count = 1 actual
'

# ── filters combined with --exclude ──────────────────────────────────────────

test_expect_success '--merged combined with --exclude' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=main --exclude=refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '--contains combined with --exclude' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --exclude=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at combined with --exclude' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --points-at=main --exclude=refs/heads/main >actual &&
	test_cmp expect actual
'

# ── filters combined with --sort + --count ───────────────────────────────────

test_expect_success '--merged + --sort + --count combined' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --merged=main --sort=-refname --count=2 >actual &&
	test_cmp expect actual
'

test_expect_success '--contains + --sort + --count combined' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --contains=refs/tags/two --sort=-refname --count=2 >actual &&
	test_cmp expect actual
'

test_done

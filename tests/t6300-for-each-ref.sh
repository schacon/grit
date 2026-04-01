#!/bin/sh
# Ported subset from git/t/t6300-for-each-ref.sh.

test_description='for-each-ref basic behaviors'

. ./test-lib.sh

setup_graph () {
	git init repo &&
	cd repo &&

	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&

	A=$(git commit-tree "$EMPTY_TREE" -m A) &&
	B=$(git commit-tree "$EMPTY_TREE" -p "$A" -m B) &&
	C=$(git commit-tree "$EMPTY_TREE" -p "$B" -m C) &&
	D=$(git commit-tree "$EMPTY_TREE" -p "$B" -m D) &&

	git update-ref refs/heads/main "$C" &&
	git update-ref refs/heads/side "$D" &&
	git update-ref refs/odd/spot "$C" &&
	git update-ref refs/tags/one "$A" &&
	git update-ref refs/tags/two "$B" &&
	git update-ref refs/tags/three "$C" &&
	git update-ref refs/tags/four "$D"
}

test_expect_success 'setup history and refs' '
	setup_graph
'

test_expect_success 'for-each-ref help text is available' '
	cd repo &&
	git for-each-ref --help >usage 2>&1 &&
	test -s usage
'

test_expect_success 'default ordering by refname' '
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
	git for-each-ref --format="%(refname)" >actual &&
	test_cmp expect actual
'

test_expect_success 'descending sort and count' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --sort=-refname --count=2 >actual &&
	test_cmp expect actual
'

test_expect_success 'prefix patterns and --exclude' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" refs/tags --exclude=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin patterns work' '
	cd repo &&
	cat >patterns <<-\EOF &&
refs/heads/*
refs/tags/t*
EOF
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_cmp expect actual
'

# ── %(objecttype) ────────────────────────────────────────────────────────────

test_expect_success '%(objecttype) for commit ref' '
	cd repo &&
	echo "commit" >expect &&
	git for-each-ref --format="%(objecttype)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success '%(objecttype) for all refs shows commit' '
	cd repo &&
	git for-each-ref --format="%(objecttype)" refs/heads >actual &&
	while IFS= read -r line; do
		test "$line" = "commit" || return 1
	done <actual
'

# ── %(objectname) ────────────────────────────────────────────────────────────

test_expect_success '%(objectname) is full 40-char hex' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/heads/main >actual &&
	oid=$(cat actual) &&
	test "$(echo "$oid" | wc -c | tr -d " ")" -eq 41 &&
	echo "$oid" | grep "^[0-9a-f]\{40\}$"
'

test_expect_success '%(objectname) matches rev-parse' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/heads/main >actual &&
	git rev-parse refs/heads/main >expect &&
	test_cmp expect actual
'

test_expect_success '%(objectname) for different refs differs when pointing at different commits' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/heads/main >oid_main &&
	git for-each-ref --format="%(objectname)" refs/heads/side >oid_side &&
	! test_cmp oid_main oid_side
'

test_expect_success '%(objectname) for refs pointing to same commit are equal' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/heads/main >oid_main &&
	git for-each-ref --format="%(objectname)" refs/odd/spot >oid_spot &&
	test_cmp oid_main oid_spot
'

# ── %(refname:short) ────────────────────────────────────────────────────────

test_expect_success '%(refname:short) strips refs/heads/ for branches' '
	cd repo &&
	echo "main" >expect &&
	git for-each-ref --format="%(refname:short)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success '%(refname:short) strips refs/tags/ for tags' '
	cd repo &&
	echo "one" >expect &&
	git for-each-ref --format="%(refname:short)" refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '%(refname:short) for non-standard namespace' '
	cd repo &&
	git for-each-ref --format="%(refname:short)" refs/odd/spot >actual &&
	test -s actual
'

test_expect_success '%(refname:short) for all heads' '
	cd repo &&
	cat >expect <<-\EOF &&
main
side
EOF
	git for-each-ref --format="%(refname:short)" refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '%(refname:short) for all tags' '
	cd repo &&
	cat >expect <<-\EOF &&
four
one
three
two
EOF
	git for-each-ref --format="%(refname:short)" refs/tags >actual &&
	test_cmp expect actual
'

# ── %(subject) ───────────────────────────────────────────────────────────────

test_expect_success '%(subject) shows commit message subject' '
	cd repo &&
	echo "C" >expect &&
	git for-each-ref --format="%(subject)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success '%(subject) for each tag matches commit message' '
	cd repo &&
	cat >expect <<-\EOF &&
D
A
C
B
EOF
	git for-each-ref --format="%(subject)" refs/tags >actual &&
	test_cmp expect actual
'

# ── format with literal text ─────────────────────────────────────────────────

test_expect_success 'format with literal text around atoms' '
	cd repo &&
	echo "ref=main type=commit" >expect &&
	git for-each-ref --format="ref=%(refname:short) type=%(objecttype)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'format with refname and objecttype combined' '
	cd repo &&
	cat >expect <<-\EOF &&
commit refs/heads/main
commit refs/heads/side
EOF
	git for-each-ref --format="%(objecttype) %(refname)" refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success 'format with multiple atoms and separators' '
	cd repo &&
	cat >expect <<-\EOF &&
main|commit|C
side|commit|D
EOF
	git for-each-ref --format="%(refname:short)|%(objecttype)|%(subject)" refs/heads >actual &&
	test_cmp expect actual
'

# ── --sort ───────────────────────────────────────────────────────────────────

test_expect_success '--sort=refname (ascending, default)' '
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
	git for-each-ref --format="%(refname)" --sort=refname >actual &&
	test_cmp expect actual
'

test_expect_success '--sort=-refname (descending)' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/one
refs/tags/four
refs/odd/spot
refs/heads/side
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --sort=-refname >actual &&
	test_cmp expect actual
'

test_expect_success '--sort=objecttype groups by type' '
	cd repo &&
	git for-each-ref --format="%(objecttype)" --sort=objecttype >actual &&
	while IFS= read -r line; do
		test "$line" = "commit" || return 1
	done <actual
'

test_expect_success '--sort=-refname with --count combines correctly' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --sort=-refname --count=3 >actual &&
	test_cmp expect actual
'

test_expect_success '--sort=refname with --count=1 gives first alphabetically' '
	cd repo &&
	echo "refs/heads/main" >expect &&
	git for-each-ref --format="%(refname)" --sort=refname --count=1 >actual &&
	test_cmp expect actual
'

test_expect_success '--sort=-refname with --count=1 gives last alphabetically' '
	cd repo &&
	echo "refs/tags/two" >expect &&
	git for-each-ref --format="%(refname)" --sort=-refname --count=1 >actual &&
	test_cmp expect actual
'

# ── --count ──────────────────────────────────────────────────────────────────

test_expect_success '--count=1 shows exactly one ref' '
	cd repo &&
	git for-each-ref --format="%(refname)" --count=1 >actual &&
	test_line_count = 1 actual
'

test_expect_success '--count=3 shows exactly three refs' '
	cd repo &&
	git for-each-ref --format="%(refname)" --count=3 >actual &&
	test_line_count = 3 actual
'

test_expect_success '--count larger than total shows all' '
	cd repo &&
	git for-each-ref --format="%(refname)" --count=100 >actual &&
	test_line_count = 7 actual
'

test_expect_success '--count=0 is accepted' '
	cd repo &&
	git for-each-ref --format="%(refname)" --count=0 >actual
'

test_expect_success '--count with --sort picks top N after sort' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --sort=-refname --count=3 >actual &&
	test_cmp expect actual
'

# ── pattern matching ─────────────────────────────────────────────────────────

test_expect_success 'pattern refs/heads matches only heads' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern refs/tags matches only tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern refs/odd matches odd namespace' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/odd/spot
EOF
	git for-each-ref --format="%(refname)" refs/odd >actual &&
	test_cmp expect actual
'

test_expect_success 'glob pattern refs/tags/t* matches t-prefixed tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" "refs/tags/t*" >actual &&
	test_cmp expect actual
'

test_expect_success 'glob pattern refs/tags/o* matches one tag' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" "refs/tags/o*" >actual &&
	test_cmp expect actual
'

test_expect_success 'glob pattern refs/heads/m* matches main only' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" "refs/heads/m*" >actual &&
	test_cmp expect actual
'

test_expect_success 'glob pattern refs/heads/s* matches side only' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" "refs/heads/s*" >actual &&
	test_cmp expect actual
'

test_expect_success 'glob pattern refs/tags/f* matches four only' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" "refs/tags/f*" >actual &&
	test_cmp expect actual
'

test_expect_success 'non-matching pattern returns empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" refs/nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success 'non-matching glob pattern returns empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" "refs/tags/z*" >actual &&
	test_must_be_empty actual
'

# ── multiple patterns ────────────────────────────────────────────────────────

test_expect_success 'multiple patterns combine results' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
EOF
	git for-each-ref --format="%(refname)" refs/heads refs/odd >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple patterns: heads + tags' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" refs/heads refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple patterns: tags + odd' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/odd/spot
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" refs/tags refs/odd >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple glob patterns' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" "refs/heads/m*" "refs/tags/o*" >actual &&
	test_cmp expect actual
'

# ── --exclude ────────────────────────────────────────────────────────────────

test_expect_success '--exclude removes matching refs' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" refs/heads --exclude=refs/heads/side >actual &&
	test_cmp expect actual
'

test_expect_success '--exclude with multiple exclusions' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" refs/tags --exclude=refs/tags/one --exclude=refs/tags/two >actual &&
	test_cmp expect actual
'

test_expect_success '--exclude all refs gives empty output' '
	cd repo &&
	git for-each-ref --format="%(refname)" refs/heads --exclude=refs/heads/main --exclude=refs/heads/side >actual &&
	test_must_be_empty actual
'

test_expect_success '--exclude non-matching ref has no effect' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" refs/heads --exclude=refs/heads/nonexistent >actual &&
	test_cmp expect actual
'

# ── --stdin ──────────────────────────────────────────────────────────────────

test_expect_success '--stdin with single pattern' '
	cd repo &&
	echo "refs/heads/*" >patterns &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin with multiple patterns' '
	cd repo &&
	cat >patterns <<-\EOF &&
refs/heads/*
refs/tags/o*
EOF
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin with non-glob patterns' '
	cd repo &&
	cat >patterns <<-\EOF &&
refs/heads
refs/odd
EOF
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
EOF
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_cmp expect actual
'

# ── annotated tags ───────────────────────────────────────────────────────────

test_expect_success 'setup annotated tag' '
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	git tag -a -m "tag annotation" v1.0 refs/heads/main
'

test_expect_success '%(objecttype) is tag for annotated tag' '
	cd repo &&
	echo "tag" >expect &&
	git for-each-ref --format="%(objecttype)" refs/tags/v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success '%(objecttype) is commit for lightweight tag' '
	cd repo &&
	echo "commit" >expect &&
	git for-each-ref --format="%(objecttype)" refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success '%(subject) shows tag annotation for annotated tag' '
	cd repo &&
	echo "tag annotation" >expect &&
	git for-each-ref --format="%(subject)" refs/tags/v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success '%(subject) shows commit subject for lightweight tag' '
	cd repo &&
	echo "A" >expect &&
	git for-each-ref --format="%(subject)" refs/tags/one >actual &&
	test_cmp expect actual
'

# ── combined features ────────────────────────────────────────────────────────

test_expect_success 'pattern + sort + count combined' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/v1.0
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --sort=-refname --count=2 refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern + exclude + sort combined' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/v1.0
refs/tags/two
refs/tags/one
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --sort=-refname refs/tags --exclude=refs/tags/three >actual &&
	test_cmp expect actual
'

test_expect_success 'format with all supported atoms' '
	cd repo &&
	git for-each-ref --format="%(refname) %(refname:short) %(objecttype) %(objectname) %(subject)" refs/heads/main >actual &&
	test -s actual &&
	# Should contain all parts
	grep "refs/heads/main main commit" actual
'

test_expect_success 'no refs at all in empty repo' '
	git init empty &&
	cd empty &&
	git for-each-ref --format="%(refname)" >actual &&
	test_must_be_empty actual
'

test_done

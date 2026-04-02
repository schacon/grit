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

# ── error handling ────────────────────────────────────────────────────────────

test_expect_success 'invalid atom name is an error when refs exist' '
	cd repo &&
	test_must_fail git for-each-ref --format="%(INVALID)" refs/heads 2>err &&
	grep "unsupported format atom" err
'

test_expect_success 'invalid atom name is fine when no refs match' '
	cd repo &&
	git for-each-ref --format="%(INVALID)" refs/does-not-exist >actual &&
	test_must_be_empty actual
'

test_expect_success 'unsupported sort key is an error' '
	cd repo &&
	test_must_fail git for-each-ref --sort=bogus 2>err &&
	grep "unsupported sort key" err
'

# ── --count edge cases ────────────────────────────────────────────────────────

test_expect_success '--count=0 gives empty output' '
	cd repo &&
	git for-each-ref --format="%(refname)" --count=0 >actual &&
	test_must_be_empty actual
'

test_expect_success '--count=1 with default sort gives first ref' '
	cd repo &&
	echo "refs/heads/main" >expect &&
	git for-each-ref --format="%(refname)" --count=1 >actual &&
	test_cmp expect actual
'

test_expect_success 'negative --count is an error' '
	cd repo &&
	test_must_fail git for-each-ref --format="%(refname)" --count=-1 2>err &&
	grep "invalid" err
'

# ── --stdin edge cases ────────────────────────────────────────────────────────

test_expect_success '--stdin: empty input matches all refs' '
	cd repo &&
	git for-each-ref --format="%(refname)" >expect &&
	git for-each-ref --format="%(refname)" --stdin </dev/null >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin: fails if extra args supplied' '
	cd repo &&
	test_must_fail git for-each-ref --format="%(refname)" \
		--stdin refs/heads/extra </dev/null 2>err &&
	grep "unknown arguments supplied with --stdin" err
'

test_expect_success '--stdin: non-existing refs gives empty output' '
	cd repo &&
	echo "refs/heads/this-ref-does-not-exist" >patterns &&
	git for-each-ref --format="%(refname)" --stdin <patterns >actual &&
	test_must_be_empty actual
'

# ── --ignore-case ─────────────────────────────────────────────────────────────

test_expect_success '--ignore-case matches case-insensitively' '
	cd repo &&
	echo "refs/heads/main" >expect &&
	git for-each-ref --format="%(refname)" --ignore-case refs/heads/MAIN >actual &&
	test_cmp expect actual
'

test_expect_success 'without --ignore-case, wrong case gives empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" refs/heads/MAIN >actual &&
	test_must_be_empty actual
'

# ── pattern matching refinements ──────────────────────────────────────────────

test_expect_success 'exact refname as pattern matches that ref' '
	cd repo &&
	echo "refs/heads/main" >expect &&
	git for-each-ref --format="%(refname)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'prefix pattern with trailing slash matches subtree' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/one
refs/tags/three
refs/tags/two
refs/tags/v1.0
EOF
	git for-each-ref --format="%(refname)" refs/tags/ >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern exclusion with glob removes matching refs' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/v1.0
EOF
	git for-each-ref --format="%(refname)" refs/tags \
		--exclude="refs/tags/o*" --exclude="refs/tags/t*" >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple --exclude patterns all apply' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/v1.0
EOF
	git for-each-ref --format="%(refname)" \
		--exclude="refs/tags/one" --exclude="refs/tags/two" \
		--exclude="refs/tags/three" --exclude="refs/tags/four" >actual &&
	test_cmp expect actual
'

# ── sorting by different keys ─────────────────────────────────────────────────

test_expect_success '--sort=objectname sorts by hash' '
	cd repo &&
	git for-each-ref --format="%(objectname) %(refname)" --sort=objectname refs/heads >actual &&
	# Extract just the hashes and verify they are sorted
	cut -d" " -f1 <actual >hashes &&
	sort <hashes >sorted_hashes &&
	test_cmp sorted_hashes hashes
'

test_expect_success '--sort=-objectname reverses objectname sort' '
	cd repo &&
	git for-each-ref --format="%(objectname)" --sort=objectname refs/heads >ascending &&
	git for-each-ref --format="%(objectname)" --sort=-objectname refs/heads >descending &&
	sort -r <ascending >expect &&
	test_cmp expect descending
'

test_expect_success '--sort=objecttype sorts by type string' '
	cd repo &&
	git for-each-ref --format="%(objecttype) %(refname)" --sort=objecttype >actual &&
	# commit sorts before tag, verify ordering
	cut -d" " -f1 <actual >types &&
	sort <types >sorted_types &&
	test_cmp sorted_types types
'

test_expect_success 'multiple sort keys: --sort=objecttype --sort=-refname' '
	cd repo &&
	git for-each-ref --format="%(objecttype) %(refname)" \
		--sort=objecttype --sort=-refname >actual &&
	# All commit entries should come before tag entries
	grep "^commit" actual >commits &&
	grep "^tag" actual >tags 2>/dev/null || true &&
	if test -s tags; then
		last_commit=$(tail -1 commits) &&
		first_tag=$(head -1 tags) &&
		# commit < tag lexicographically so commits should be first
		test "$last_commit" != "$first_tag"
	fi
'

# ── deref atoms (*subject) ────────────────────────────────────────────────────

test_expect_success '%(*subject) peels annotated tag to commit subject' '
	cd repo &&
	echo "C" >expect &&
	git for-each-ref --format="%(*subject)" refs/tags/v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success '%(*subject) on lightweight tag shows commit subject' '
	cd repo &&
	echo "A" >expect &&
	git for-each-ref --format="%(*subject)" refs/tags/one >actual &&
	test_cmp expect actual
'

test_expect_success 'nested annotated tags: %(*subject) peels to commit' '
	cd repo &&
	git tag -a -m "double" v2.0 refs/tags/v1.0 &&
	echo "C" >expect &&
	git for-each-ref --format="%(*subject)" refs/tags/v2.0 >actual &&
	test_cmp expect actual
'


# ── --points-at ────────────────────────────────────────────────────────────

test_expect_success '--points-at shows refs pointing at given commit' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
refs/tags/v1.0
refs/tags/v2.0
EOF
	git for-each-ref --format="%(refname)" --points-at="$C" >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with no matching refs gives empty' '
	cd repo &&
	ET=$(printf "" | git hash-object -w -t tree --stdin) &&
	NONE=$(git commit-tree "$ET" -m "orphan") &&
	git for-each-ref --format="%(refname)" --points-at="$NONE" >actual &&
	test_must_be_empty actual
'

test_expect_success '--points-at with tag pattern' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/three
refs/tags/v1.0
refs/tags/v2.0
EOF
	git for-each-ref --format="%(refname)" --points-at="$C" refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with heads pattern' '
	cd repo &&
	D=$(git rev-parse refs/heads/side) &&
	cat >expect <<-\EOF &&
refs/heads/side
EOF
	git for-each-ref --format="%(refname)" --points-at="$D" refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at combined with --count' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	git for-each-ref --format="%(refname)" --points-at="$C" --count=1 >actual &&
	test_line_count = 1 actual
'

test_expect_success '--points-at combined with --sort=-refname' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/v2.0
refs/tags/v1.0
refs/tags/three
refs/odd/spot
refs/heads/main
EOF
	git for-each-ref --format="%(refname)" --points-at="$C" --sort=-refname >actual &&
	test_cmp expect actual
'

# ── --contains / --no-contains ─────────────────────────────────────────────

test_expect_success '--contains shows refs whose tip contains given commit' '
	cd repo &&
	B=$(git rev-parse refs/tags/two) &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/heads/side
refs/odd/spot
refs/tags/four
refs/tags/three
refs/tags/two
refs/tags/v1.0
refs/tags/v2.0
EOF
	git for-each-ref --format="%(refname)" --contains="$B" \
		refs/heads refs/tags refs/odd >actual &&
	test_cmp expect actual
'

test_expect_success '--contains with root commit includes all lightweight tag refs' '
	cd repo &&
	A=$(git rev-parse refs/tags/one) &&
	git for-each-ref --format="%(refname)" --contains="$A" \
		refs/heads refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_line_count -gt 0 actual
'

test_expect_success '--no-contains excludes refs that contain given commit' '
	cd repo &&
	B=$(git rev-parse refs/tags/two) &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --no-contains="$B" \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--contains with tip commit shows refs containing it' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --contains="$C" \
		refs/heads refs/odd refs/tags/three >actual &&
	test_cmp expect actual
'

# ── --merged / --no-merged ────────────────────────────────────────────────────

test_expect_success '--merged shows refs reachable from given commit' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/one
refs/tags/three
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged="$C" \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged shows refs not reachable from given commit' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --no-merged="$C" \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--merged with branch name' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
EOF
	git for-each-ref --format="%(refname)" --merged=main \
		refs/heads/main refs/odd >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged with branch shows diverged refs' '
	cd repo &&
	echo "refs/heads/side" >expect &&
	git for-each-ref --format="%(refname)" --no-merged=main refs/heads >actual &&
	test_cmp expect actual
'

test_expect_success '--merged combined with --sort=-refname' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/two
refs/tags/three
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --merged="$C" --sort=-refname \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--merged combined with --count' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	git for-each-ref --format="%(refname)" --merged="$C" --count=2 \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_line_count = 2 actual
'

# ── format edge cases ─────────────────────────────────────────────────────────

test_expect_success 'empty format string produces empty lines' '
	cd repo &&
	git for-each-ref --format="" refs/heads/main >actual &&
	echo "" >expect &&
	test_cmp expect actual
'

test_expect_success 'format with only literal text' '
	cd repo &&
	echo "hello" >expect &&
	git for-each-ref --format="hello" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'format with repeated atoms' '
	cd repo &&
	echo "refs/heads/main refs/heads/main" >expect &&
	git for-each-ref --format="%(refname) %(refname)" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'format with literal before and after atom' '
	cd repo &&
	echo "[refs/heads/main]" >expect &&
	git for-each-ref --format="[%(refname)]" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'format with multiple literals and atoms' '
	cd repo &&
	echo "<main> is a <commit>" >expect &&
	git for-each-ref --format="<%(refname:short)> is a <%(objecttype)>" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'format with tab characters' '
	cd repo &&
	printf "main\tcommit\n" >expect &&
	git for-each-ref --format="%(refname:short)	%(objecttype)" refs/heads/main >actual &&
	test_cmp expect actual
'

# ── sort by objecttype ────────────────────────────────────────────────────────

test_expect_success '--sort=objecttype puts commits before tags' '
	cd repo &&
	cat >expect <<-\EOF &&
commit refs/heads/main
commit refs/heads/side
commit refs/odd/spot
commit refs/tags/four
commit refs/tags/one
commit refs/tags/three
commit refs/tags/two
tag refs/tags/v1.0
tag refs/tags/v2.0
EOF
	git for-each-ref --format="%(objecttype) %(refname)" --sort=objecttype >actual &&
	test_cmp expect actual
'

test_expect_success '--sort=-objecttype puts tags before commits' '
	cd repo &&
	cat >expect <<-\EOF &&
tag refs/tags/v1.0
tag refs/tags/v2.0
commit refs/heads/main
commit refs/heads/side
commit refs/odd/spot
commit refs/tags/four
commit refs/tags/one
commit refs/tags/three
commit refs/tags/two
EOF
	git for-each-ref --format="%(objecttype) %(refname)" --sort=-objecttype >actual &&
	test_cmp expect actual
'

# ── --merged with different branches ──────────────────────────────────────────

test_expect_success '--merged=side shows refs merged into side' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/four
refs/tags/one
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --merged=side \
		refs/tags/one refs/tags/two refs/tags/three refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--no-merged=side shows refs not merged into side' '
	cd repo &&
	cat >expect <<-\EOF &&
refs/tags/three
refs/tags/v1.0
refs/tags/v2.0
EOF
	git for-each-ref --format="%(refname)" --no-merged=side refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success '--merged and --no-merged combined narrows results' '
	cd repo &&
	echo "refs/heads/main" >expect &&
	git for-each-ref --format="%(refname)" --merged=main --no-merged=side refs/heads >actual &&
	test_cmp expect actual
'

# ── --contains / --no-contains combos ─────────────────────────────────────────

test_expect_success '--contains + --no-contains narrows results' '
	cd repo &&
	B=$(git rev-parse refs/tags/two) &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
refs/tags/two
EOF
	git for-each-ref --format="%(refname)" --contains="$B" --no-contains="$C" \
		refs/heads refs/tags/two refs/tags/four >actual &&
	test_cmp expect actual
'

test_expect_success '--no-contains + --no-merged combined' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --no-contains="$C" --no-merged=main refs/tags >actual &&
	test_cmp expect actual
'

# ── --points-at with --exclude ────────────────────────────────────────────────

test_expect_success '--points-at combined with --exclude' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	cat >expect <<-\EOF &&
refs/heads/main
refs/odd/spot
refs/tags/three
EOF
	git for-each-ref --format="%(refname)" --points-at="$C" \
		--exclude=refs/tags/v1.0 --exclude=refs/tags/v2.0 >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with different commit' '
	cd repo &&
	D=$(git rev-parse refs/heads/side) &&
	cat >expect <<-\EOF &&
refs/heads/side
refs/tags/four
EOF
	git for-each-ref --format="%(refname)" --points-at="$D" >actual &&
	test_cmp expect actual
'

test_expect_success '--points-at with root commit' '
	cd repo &&
	A=$(git rev-parse refs/tags/one) &&
	cat >expect <<-\EOF &&
refs/tags/one
EOF
	git for-each-ref --format="%(refname)" --points-at="$A" >actual &&
	test_cmp expect actual
'

# ── --merged + --contains combo ───────────────────────────────────────────────

test_expect_success '--merged + --contains combined' '
	cd repo &&
	B=$(git rev-parse refs/tags/two) &&
	cat >expect <<-\EOF &&
refs/tags/three
refs/tags/two
refs/tags/v1.0
refs/tags/v2.0
EOF
	git for-each-ref --format="%(refname)" --merged=main --contains="$B" refs/tags >actual &&
	test_cmp expect actual
'

# ── subject on various object types ───────────────────────────────────────────

test_expect_success '%(subject) shows tag annotation for annotated tags' '
	cd repo &&
	cat >expect <<-\EOF &&
tag annotation refs/tags/v1.0
double refs/tags/v2.0
EOF
	git for-each-ref --format="%(subject) %(refname)" refs/tags/v1.0 refs/tags/v2.0 >actual &&
	test_cmp expect actual
'

test_expect_success '%(subject) for all tags' '
	cd repo &&
	cat >expect <<-\EOF &&
D refs/tags/four
A refs/tags/one
C refs/tags/three
B refs/tags/two
tag annotation refs/tags/v1.0
double refs/tags/v2.0
EOF
	git for-each-ref --format="%(subject) %(refname)" refs/tags >actual &&
	test_cmp expect actual
'

test_expect_success '%(subject) for heads matches commit messages' '
	cd repo &&
	cat >expect <<-\EOF &&
C refs/heads/main
D refs/heads/side
EOF
	git for-each-ref --format="%(subject) %(refname)" refs/heads >actual &&
	test_cmp expect actual
'

# ── objectname consistency checks ─────────────────────────────────────────────

test_expect_success '%(objectname) for annotated tag differs from peeled commit' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/tags/v1.0 >tag_oid &&
	git for-each-ref --format="%(objectname)" refs/heads/main >commit_oid &&
	! test_cmp tag_oid commit_oid
'

test_expect_success '%(objectname) is consistent across runs' '
	cd repo &&
	git for-each-ref --format="%(objectname)" refs/heads/main >run1 &&
	git for-each-ref --format="%(objectname)" refs/heads/main >run2 &&
	test_cmp run1 run2
'

# ── edge cases: no matching refs ──────────────────────────────────────────────

test_expect_success '--merged with no matching refs gives empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" --merged=main refs/nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success '--contains with no matching refs gives empty' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	git for-each-ref --format="%(refname)" --contains="$C" refs/nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success '--points-at with no matching refs gives empty' '
	cd repo &&
	C=$(git rev-parse refs/heads/main) &&
	git for-each-ref --format="%(refname)" --points-at="$C" refs/nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success '--no-merged with all-merged refs gives empty' '
	cd repo &&
	git for-each-ref --format="%(refname)" --no-merged=main refs/heads/main >actual &&
	test_must_be_empty actual
'

test_expect_success '--no-contains with all-contained refs gives empty' '
	cd repo &&
	A=$(git rev-parse refs/tags/one) &&
	git for-each-ref --format="%(refname)" --no-contains="$A" refs/tags/one >actual &&
	test_must_be_empty actual
'

test_done

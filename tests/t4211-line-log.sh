#!/bin/sh
# Tests for 'grit rev-list' — commit enumeration and range queries.
# (log -L line-range is not yet implemented; these tests cover rev-list
# which is the underlying commit-walk engine.)

test_description='grit rev-list'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup linear history' '
	git init repo &&
	cd repo &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m "c1" &&
	git tag v1 &&

	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m "c2" &&
	git tag v2 &&

	echo three >file &&
	git add file &&
	test_tick &&
	git commit -m "c3" &&
	git tag v3 &&

	echo four >file &&
	git add file &&
	test_tick &&
	git commit -m "c4" &&
	git tag v4 &&

	echo five >file &&
	git add file &&
	test_tick &&
	git commit -m "c5" &&
	git tag v5
'

# ── Basic listing ────────────────────────────────────────────────────────────

test_expect_success 'rev-list HEAD lists all commits' '
	cd repo &&
	git rev-list HEAD >actual &&
	test_line_count = 5 actual
'

test_expect_success 'rev-list outputs full 40-char hashes' '
	cd repo &&
	git rev-list HEAD >actual &&
	while read hash; do
		test $(echo "$hash" | wc -c) -eq 41 || return 1
	done <actual
'

test_expect_success 'rev-list HEAD matches rev-parse HEAD as first entry' '
	cd repo &&
	git rev-list HEAD >actual &&
	head -1 actual >first &&
	git rev-parse HEAD >expect &&
	test_cmp expect first
'

test_expect_success 'rev-list with tag name' '
	cd repo &&
	git rev-list v3 >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-list with tag vs HEAD gives subset' '
	cd repo &&
	git rev-list v3 >v3_list &&
	git rev-list HEAD >head_list &&
	# v3 commits should be a subset of HEAD commits
	while read hash; do
		grep -q "$hash" head_list || return 1
	done <v3_list
'

# ── --count ──────────────────────────────────────────────────────────────────

test_expect_success 'rev-list --count HEAD' '
	cd repo &&
	RESULT=$(git rev-list --count HEAD) &&
	test "$RESULT" = "5"
'

test_expect_success 'rev-list --count with tag' '
	cd repo &&
	RESULT=$(git rev-list --count v3) &&
	test "$RESULT" = "3"
'

test_expect_success 'rev-list --count v1 is 1' '
	cd repo &&
	RESULT=$(git rev-list --count v1) &&
	test "$RESULT" = "1"
'

# ── --max-count / -n ─────────────────────────────────────────────────────────

test_expect_success 'rev-list --max-count=2 limits output' '
	cd repo &&
	git rev-list --max-count=2 HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list -n 1 gives single commit' '
	cd repo &&
	git rev-list -n 1 HEAD >actual &&
	test_line_count = 1 actual
'

test_expect_success 'rev-list -n 0 gives no output' '
	cd repo &&
	git rev-list -n 0 HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'rev-list -n larger than total gives all' '
	cd repo &&
	git rev-list -n 100 HEAD >actual &&
	test_line_count = 5 actual
'

# ── --reverse ────────────────────────────────────────────────────────────────

test_expect_success 'rev-list --reverse reverses order' '
	cd repo &&
	git rev-list HEAD >normal &&
	git rev-list --reverse HEAD >reversed &&
	tail -1 normal >last_normal &&
	head -1 reversed >first_reversed &&
	test_cmp last_normal first_reversed
'

test_expect_success 'rev-list --reverse first is root commit' '
	cd repo &&
	git rev-list --reverse HEAD >actual &&
	head -1 actual >first &&
	git rev-parse v1 >expect &&
	test_cmp expect first
'

test_expect_success 'rev-list --reverse last is HEAD' '
	cd repo &&
	git rev-list --reverse HEAD >actual &&
	tail -1 actual >last &&
	git rev-parse HEAD >expect &&
	test_cmp expect last
'

# ── Range notation (A..B) ───────────────────────────────────────────────────

test_expect_success 'rev-list A..B shows commits in B not in A' '
	cd repo &&
	git rev-list v3..v5 >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list A..A is empty' '
	cd repo &&
	git rev-list v3..v3 >actual &&
	test_must_be_empty actual
'

test_expect_success 'rev-list v1..HEAD shows all but first' '
	cd repo &&
	git rev-list v1..HEAD >actual &&
	test_line_count = 4 actual
'

test_expect_success 'rev-list range does not include boundary commit' '
	cd repo &&
	V1_HASH=$(git rev-parse v1) &&
	git rev-list v1..HEAD >actual &&
	! grep -q "$V1_HASH" actual
'

test_expect_success 'rev-list v4..v5 gives exactly one commit' '
	cd repo &&
	git rev-list v4..v5 >actual &&
	test_line_count = 1 actual &&
	V5_HASH=$(git rev-parse v5) &&
	echo "$V5_HASH" >expect &&
	test_cmp expect actual
'

# ── --all ────────────────────────────────────────────────────────────────────

test_expect_success 'rev-list --all lists all reachable commits' '
	cd repo &&
	git rev-list --all >actual &&
	test_line_count = 5 actual
'

test_expect_success 'setup side branch for --all test' '
	cd repo &&
	HASH_C3=$(git rev-parse v3) &&
	git branch side "$HASH_C3" &&
	git checkout side &&
	echo side >side-file &&
	git add side-file &&
	test_tick &&
	git commit -m "side1" &&
	git checkout master
'

test_expect_success 'rev-list --all includes side branch commits' '
	cd repo &&
	git rev-list --all >actual &&
	test_line_count = 6 actual
'

test_expect_success 'rev-list HEAD still shows only 5 on master' '
	cd repo &&
	git checkout master &&
	git rev-list HEAD >actual &&
	test_line_count = 5 actual
'

# ── --count with ranges ─────────────────────────────────────────────────────

test_expect_success 'rev-list --count with range' '
	cd repo &&
	RESULT=$(git rev-list --count v2..v5) &&
	test "$RESULT" = "3"
'

test_expect_success 'rev-list --count A..A is 0' '
	cd repo &&
	RESULT=$(git rev-list --count v3..v3) &&
	test "$RESULT" = "0"
'

# ── Verify consistency ──────────────────────────────────────────────────────

test_expect_success 'rev-list output hashes are valid objects' '
	cd repo &&
	git rev-list HEAD >hashes &&
	while read hash; do
		git cat-file -t "$hash" >type &&
		echo "commit" >expect &&
		test_cmp expect type || return 1
	done <hashes
'

test_expect_success 'rev-list order is reverse chronological' '
	cd repo &&
	git rev-list HEAD >hashes &&
	PREV="" &&
	while read hash; do
		if test -n "$PREV"; then
			# Current should be parent of previous or older
			git cat-file commit "$PREV" >info &&
			grep -q "$hash" info || true
		fi
		PREV="$hash"
	done <hashes
'

test_done

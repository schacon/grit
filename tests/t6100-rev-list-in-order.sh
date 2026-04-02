#!/bin/sh
# Test rev-list output ordering: chronological, topo, reverse, --count, etc.

test_description='rev-list output ordering'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL='author@example.com'
GIT_COMMITTER_NAME='C O Mmiter'
GIT_COMMITTER_EMAIL='committer@example.com'
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

# Helper to create a commit with a specific timestamp
make_commit () {
	local msg="$1"
	local ts="$2"
	GIT_COMMITTER_DATE="$ts +0000" GIT_AUTHOR_DATE="$ts +0000" \
		export GIT_COMMITTER_DATE GIT_AUTHOR_DATE &&
	grit commit --allow-empty -m "$msg"
}

test_expect_success 'setup linear history' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "a" >a.txt &&
	grit add a.txt &&
	make_commit "first" "1000000000" &&
	echo "b" >b.txt &&
	grit add b.txt &&
	make_commit "second" "1000000100" &&
	echo "c" >c.txt &&
	grit add c.txt &&
	make_commit "third" "1000000200" &&
	echo "d" >d.txt &&
	grit add d.txt &&
	make_commit "fourth" "1000000300" &&
	echo "e" >e.txt &&
	grit add e.txt &&
	make_commit "fifth" "1000000400"
'

test_expect_success 'rev-list HEAD lists all commits' '
	cd repo &&
	grit rev-list HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "5"
'

test_expect_success 'rev-list default order: newest first' '
	cd repo &&
	grit rev-list HEAD >actual &&
	first_line=$(head -1 actual) &&
	head_sha=$(grit rev-parse HEAD) &&
	test "$first_line" = "$head_sha"
'

test_expect_success 'rev-list --reverse: oldest first' '
	cd repo &&
	grit rev-list --reverse HEAD >actual &&
	last_line=$(tail -1 actual) &&
	head_sha=$(grit rev-parse HEAD) &&
	test "$last_line" = "$head_sha"
'

test_expect_success 'rev-list --reverse is reversed default order' '
	cd repo &&
	grit rev-list HEAD >forward &&
	grit rev-list --reverse HEAD >reversed &&
	tac forward >expected &&
	test_cmp expected reversed
'

test_expect_success 'rev-list --count returns count' '
	cd repo &&
	grit rev-list --count HEAD >actual &&
	test "$(cat actual)" = "5"
'

test_expect_success 'rev-list --max-count=N limits output' '
	cd repo &&
	grit rev-list --max-count=3 HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "3"
'

test_expect_success 'rev-list --max-count=1 returns only HEAD' '
	cd repo &&
	grit rev-list --max-count=1 HEAD >actual &&
	head_sha=$(grit rev-parse HEAD) &&
	test "$(cat actual)" = "$head_sha"
'

test_expect_success 'rev-list --max-count=0 returns nothing' '
	cd repo &&
	grit rev-list --max-count=0 HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "0"
'

test_expect_success 'rev-list --topo-order on linear history' '
	cd repo &&
	grit rev-list --topo-order HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "5"
'

test_expect_success 'rev-list --date-order on linear history' '
	cd repo &&
	grit rev-list --date-order HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "5"
'

test_expect_success 'rev-list with range excludes commits' '
	cd repo &&
	FIRST=$(grit rev-list --reverse HEAD | head -1) &&
	echo "$FIRST" >../first_sha &&
	grit rev-list HEAD ^$FIRST >actual &&
	test "$(wc -l <actual | tr -d " ")" = "4"
'

test_expect_success 'rev-list A..B syntax works' '
	cd repo &&
	FIRST=$(cat ../first_sha) &&
	grit rev-list $FIRST..HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "4"
'

test_expect_success 'setup branching history' '
	cd repo &&
	SECOND=$(grit rev-list --reverse HEAD | sed -n 2p) &&
	echo "$SECOND" >../second_sha &&
	grit branch side $SECOND &&
	grit checkout side &&
	echo "side1" >s1.txt &&
	grit add s1.txt &&
	make_commit "side-1" "1000000150" &&
	echo "side2" >s2.txt &&
	grit add s2.txt &&
	make_commit "side-2" "1000000250" &&
	grit checkout master
'

test_expect_success 'rev-list branch..master shows master-only commits' '
	cd repo &&
	grit rev-list side..master >actual &&
	test "$(wc -l <actual | tr -d " ")" = "3"
'

test_expect_success 'rev-list master..branch shows branch-only commits' '
	cd repo &&
	grit rev-list master..side >actual &&
	test "$(wc -l <actual | tr -d " ")" = "2"
'

test_expect_success 'rev-list with both branches lists union' '
	cd repo &&
	grit rev-list master side >actual &&
	# 5 master + 2 side - 2 shared = 7 unique
	test "$(wc -l <actual | tr -d " ")" = "7"
'

test_expect_success 'rev-list --count with range' '
	cd repo &&
	grit rev-list --count side..master >actual &&
	test "$(cat actual)" = "3"
'

test_expect_success 'rev-list --topo-order parents before children' '
	cd repo &&
	grit rev-list --topo-order master >actual &&
	head_sha=$(grit rev-parse master) &&
	first_line=$(head -1 actual) &&
	test "$first_line" = "$head_sha"
'

test_expect_success 'rev-list --reverse --max-count combines correctly' '
	cd repo &&
	grit rev-list --reverse --max-count=2 HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" = "2"
'

test_expect_success 'rev-list --count --max-count interaction' '
	cd repo &&
	grit rev-list --count --max-count=2 HEAD >actual &&
	test "$(cat actual)" = "2"
'

test_expect_success 'rev-list single commit (root)' '
	cd repo &&
	FIRST=$(cat ../first_sha) &&
	grit rev-list $FIRST >actual &&
	test "$(wc -l <actual | tr -d " ")" = "1"
'

test_expect_success 'rev-list with tag ref' '
	cd repo &&
	grit tag v1.0 HEAD &&
	grit rev-list v1.0 >actual &&
	grit rev-list HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-list --topo-order with merge' '
	cd repo &&
	MB=$(grit merge-base master side) &&
	MBTREE=$(grit rev-parse $MB^{tree}) &&
	MTREE=$(grit rev-parse master^{tree}) &&
	STREE=$(grit rev-parse side^{tree}) &&
	grit read-tree -m $MBTREE $MTREE $STREE &&
	MERGE_TREE=$(grit write-tree) &&
	MERGE=$(grit commit-tree -m "merge" $MERGE_TREE -p master -p side) &&
	grit update-ref refs/heads/merged $MERGE &&
	grit rev-list --topo-order merged >actual &&
	# merge + 5 master + 2 side - 2 shared = 8
	test "$(wc -l <actual | tr -d " ")" = "8"
'

test_expect_success 'rev-list --date-order with merge' '
	cd repo &&
	grit rev-list --date-order merged >actual &&
	test "$(wc -l <actual | tr -d " ")" = "8"
'

test_expect_success 'rev-list --reverse with merge' '
	cd repo &&
	grit rev-list merged >forward &&
	grit rev-list --reverse merged >reversed &&
	tac forward >expected &&
	test_cmp expected reversed
'

test_expect_success 'rev-list --count with merge' '
	cd repo &&
	grit rev-list --count merged >actual &&
	test "$(cat actual)" = "8"
'

test_done

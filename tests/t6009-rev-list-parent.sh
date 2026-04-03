#!/bin/sh
# Test --min-parents, --max-parents, --merges, --no-merges for rev-list.

test_description='rev-list parent filtering (--merges, --no-merges, --min/max-parents)'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

M=1130000000
Z=+0000
export M Z

doit () {
	OFFSET=$1 &&
	NAME=$2 &&
	shift 2 &&
	PARENTS= &&
	for P
	do
		PARENTS="$PARENTS -p $P"
	done &&
	GIT_COMMITTER_DATE="$(($M + $OFFSET)) $Z" &&
	GIT_AUTHOR_DATE="$GIT_COMMITTER_DATE" &&
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE &&
	commit=$(echo "$NAME" | git commit-tree "$(git write-tree)" $PARENTS) &&
	echo "$commit"
}

# Build a topology:
#   root -- A -- B -- M (merge of B and C) -- D
#                \-- C -/
# root: 0 parents (root commit)
# A, B, C, D: 1 parent each (normal commits)
# M: 2 parents (merge commit)
test_expect_success 'setup merge and linear topology' '
	grit init repo &&
	cd repo &&
	root=$(doit 1 root) &&
	a=$(doit 2 A "$root") &&
	b=$(doit 3 B "$a") &&
	c=$(doit 4 C "$a") &&
	m=$(doit 5 M "$b" "$c") &&
	d=$(doit 6 D "$m") &&
	git update-ref refs/heads/master "$d" &&
	echo "$root" >../oid_root &&
	echo "$a" >../oid_a &&
	echo "$b" >../oid_b &&
	echo "$c" >../oid_c &&
	echo "$m" >../oid_m &&
	echo "$d" >../oid_d
'

test_expect_success 'rev-list --parents shows parent OIDs' '
	cd repo &&
	m=$(cat ../oid_m) &&
	b=$(cat ../oid_b) &&
	c=$(cat ../oid_c) &&
	git rev-list --parents HEAD >actual &&
	grep "$m $b $c" actual
'

test_expect_success '--merges shows only merge commits' '
	cd repo &&
	m=$(cat ../oid_m) &&
	git rev-list --merges HEAD >actual &&
	echo "$m" >expect &&
	test_cmp expect actual
'

test_expect_success '--no-merges excludes merge commits' '
	cd repo &&
	m=$(cat ../oid_m) &&
	git rev-list --no-merges HEAD >actual &&
	! grep "$m" actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" = "5"
'

test_expect_success '--min-parents=2 selects merges' '
	cd repo &&
	m=$(cat ../oid_m) &&
	git rev-list --min-parents=2 HEAD >actual &&
	echo "$m" >expect &&
	test_cmp expect actual
'

test_expect_success '--max-parents=0 selects root commits' '
	cd repo &&
	root=$(cat ../oid_root) &&
	git rev-list --max-parents=0 HEAD >actual &&
	echo "$root" >expect &&
	test_cmp expect actual
'

test_expect_success '--max-parents=1 excludes merges and includes rest' '
	cd repo &&
	m=$(cat ../oid_m) &&
	git rev-list --max-parents=1 HEAD >actual &&
	! grep "$m" actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" = "5"
'

test_expect_success '--min-parents=1 --max-parents=1 selects single-parent only' '
	cd repo &&
	m=$(cat ../oid_m) &&
	root=$(cat ../oid_root) &&
	git rev-list --min-parents=1 --max-parents=1 HEAD >actual &&
	! grep "$m" actual &&
	! grep "$root" actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" = "4"
'

test_expect_success '--merges --count returns 1' '
	cd repo &&
	git rev-list --merges --count HEAD >actual &&
	echo 1 >expect &&
	test_cmp expect actual
'

test_expect_success '--no-merges --count returns 5' '
	cd repo &&
	git rev-list --no-merges --count HEAD >actual &&
	echo 5 >expect &&
	test_cmp expect actual
'

test_done

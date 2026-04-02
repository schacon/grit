#!/bin/sh
# Test --ancestry-path for rev-list.

test_description='rev-list --ancestry-path'

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

# Topology:
#
#   A -- B -- E -- F (master)
#    \       /
#     C --- D (side)
#
# ancestry-path side..master should include E (merge) and F,
# but NOT B (which is reachable from A but not through side).
test_expect_success 'setup branch-and-merge topology' '
	grit init repo &&
	cd repo &&
	a=$(doit 1 A) &&
	b=$(doit 2 B "$a") &&
	c=$(doit 3 C "$a") &&
	d=$(doit 4 D "$c") &&
	e=$(doit 5 E "$b" "$d") &&
	f=$(doit 6 F "$e") &&
	git update-ref refs/heads/master "$f" &&
	git update-ref refs/heads/side "$d" &&
	echo "$a" >../oid_a &&
	echo "$b" >../oid_b &&
	echo "$c" >../oid_c &&
	echo "$d" >../oid_d &&
	echo "$e" >../oid_e &&
	echo "$f" >../oid_f
'

test_expect_success '--ancestry-path side..master excludes non-path commits' '
	cd repo &&
	b=$(cat ../oid_b) &&
	e=$(cat ../oid_e) &&
	f=$(cat ../oid_f) &&
	git rev-list --ancestry-path side..master >actual &&
	grep "$e" actual &&
	grep "$f" actual &&
	! grep "$b" actual
'

test_expect_success '--ancestry-path side..master commit count' '
	cd repo &&
	git rev-list --ancestry-path side..master >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" = "2"
'

test_expect_success 'without --ancestry-path, side..master includes B' '
	cd repo &&
	b=$(cat ../oid_b) &&
	git rev-list side..master >actual &&
	grep "$b" actual
'

test_expect_success '--ancestry-path A..master on linear portion' '
	cd repo &&
	a=$(cat ../oid_a) &&
	git rev-list --ancestry-path "$a"..master >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 3
'

test_expect_success '--ancestry-path with --count' '
	cd repo &&
	git rev-list --ancestry-path --count side..master >actual &&
	echo 2 >expect &&
	test_cmp expect actual
'

test_expect_success '--ancestry-path with --reverse' '
	cd repo &&
	e=$(cat ../oid_e) &&
	f=$(cat ../oid_f) &&
	git rev-list --ancestry-path --reverse side..master >actual &&
	head -1 actual >first &&
	echo "$e" >expect_first &&
	test_cmp expect_first first &&
	tail -1 actual >last &&
	echo "$f" >expect_last &&
	test_cmp expect_last last
'

test_done

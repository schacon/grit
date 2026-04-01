#!/bin/sh
# Ported subset from git/t/t6003-rev-list-topo-order.sh.

test_description='rev-list ordering: topo/date/reverse'

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

test_expect_success 'setup merge graph' '
	grit init repo &&
	cd repo &&
	root=$(doit 1 root) &&
	side1=$(doit 2 side1 "$root") &&
	main1=$(doit 3 main1 "$root") &&
	merge=$(doit 4 merge "$main1" "$side1") &&
	git update-ref refs/heads/master "$merge" &&
	git update-ref refs/tags/root "$root" &&
	git update-ref refs/tags/side1 "$side1" &&
	git update-ref refs/tags/main1 "$main1" &&
	git update-ref refs/tags/merge "$merge"
'

test_expect_success '--topo-order keeps parents after children' '
	cd repo &&
	git rev-list --topo-order --parents refs/heads/master >actual &&
	while read oid p1 p2 rest
	do
		test -z "$p1" && continue
		line_oid=$(grep -n "^$oid\\( \\|$\\)" actual | cut -d: -f1) || return 1
		line_p1=$(grep -n "^$p1\\( \\|$\\)" actual | cut -d: -f1) || return 1
		test "$line_oid" -lt "$line_p1" || return 1
		if test -n "$p2"
		then
			line_p2=$(grep -n "^$p2\\( \\|$\\)" actual | cut -d: -f1) || return 1
			test "$line_oid" -lt "$line_p2" || return 1
		fi
	done <actual
'

test_expect_success '--date-order keeps parent-after-child constraint' '
	cd repo &&
	git rev-list --date-order --parents refs/heads/master >actual &&
	while read oid p1 p2 rest
	do
		test -z "$p1" && continue
		line_oid=$(grep -n "^$oid\\( \\|$\\)" actual | cut -d: -f1) || return 1
		line_p1=$(grep -n "^$p1\\( \\|$\\)" actual | cut -d: -f1) || return 1
		test "$line_oid" -lt "$line_p1" || return 1
		if test -n "$p2"
		then
			line_p2=$(grep -n "^$p2\\( \\|$\\)" actual | cut -d: -f1) || return 1
			test "$line_oid" -lt "$line_p2" || return 1
		fi
	done <actual
'

test_expect_success '--reverse reverses selected list' '
	cd repo &&
	git rev-list --max-count=3 refs/heads/master >forward &&
	git rev-list --max-count=3 --reverse refs/heads/master >reversed &&
	awk "{ lines[NR] = \$0 } END { for (i = NR; i >= 1; i--) print lines[i] }" forward >expect &&
	test_cmp expect reversed
'

test_expect_success 'setup extended graph' '
	cd repo &&
	merge_sha=$(git rev-parse merge) &&
	l3=$(doit 5 l3 "$merge_sha") &&
	l4=$(doit 6 l4 "$l3") &&
	l5=$(doit 7 l5 "$l4") &&
	git update-ref refs/heads/master "$l5" &&
	git update-ref refs/tags/l3 "$l3" &&
	git update-ref refs/tags/l4 "$l4" &&
	git update-ref refs/tags/l5 "$l5"
'

test_expect_success 'rev-list has correct number of entries' '
	cd repo &&
	lines=$(git rev-list refs/heads/master | wc -l | tr -d " ") &&
	test "$lines" = "7"
'

test_expect_success '--topo-order with pruning' '
	cd repo &&
	git rev-list --topo-order merge..l5 >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "3"
'

test_expect_success 'head has no parent: single commit walk' '
	cd repo &&
	git rev-list --topo-order root >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "1"
'

test_expect_success 'linear prune l5 ^merge via topo' '
	cd repo &&
	git rev-list --topo-order l5 ^merge >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "3"
'

test_expect_success '--max-count with --topo-order' '
	cd repo &&
	git rev-list --topo-order --max-count=4 refs/heads/master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "4"
'

test_expect_success '--max-count without --topo-order' '
	cd repo &&
	git rev-list --max-count=4 refs/heads/master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "4"
'

test_expect_success 'duplicated head arguments produce no duplicates' '
	cd repo &&
	git rev-list --topo-order l5 l5 >actual &&
	sort actual >sorted &&
	uniq -d sorted >dups &&
	test_must_be_empty dups
'

test_expect_success 'head ^head yields empty output' '
	cd repo &&
	git rev-list --topo-order l5 ^l5 >actual &&
	test_must_be_empty actual
'

test_expect_success '--reverse with full walk' '
	cd repo &&
	git rev-list refs/heads/master >forward &&
	git rev-list --reverse refs/heads/master >reversed &&
	awk "{ lines[NR] = \$0 } END { for (i = NR; i >= 1; i--) print lines[i] }" forward >expect &&
	test_cmp expect reversed
'

test_expect_success 'multiple heads combine correctly without duplicates' '
	cd repo &&
	git rev-list side1 main1 >actual &&
	sort actual >sorted &&
	uniq -d sorted >dups &&
	test_must_be_empty dups
'

test_done

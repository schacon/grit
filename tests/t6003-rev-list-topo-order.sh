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
	gust init repo &&
	cd repo &&
	root=$(doit 1 root) &&
	side1=$(doit 2 side1 "$root") &&
	main1=$(doit 3 main1 "$root") &&
	merge=$(doit 4 merge "$main1" "$side1") &&
	git update-ref refs/heads/main "$merge"
'

test_expect_success '--topo-order keeps parents after children' '
	cd repo &&
	git rev-list --topo-order --parents refs/heads/main >actual &&
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
	git rev-list --date-order --parents refs/heads/main >actual &&
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
	git rev-list --max-count=3 refs/heads/main >forward &&
	git rev-list --max-count=3 --reverse refs/heads/main >reversed &&
	awk "{ lines[NR] = \$0 } END { for (i = NR; i >= 1; i--) print lines[i] }" forward >expect &&
	test_cmp expect reversed
'

test_done

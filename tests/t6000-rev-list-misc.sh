#!/bin/sh
# Ported subset from git/t/t6000-rev-list-misc.sh.

test_description='miscellaneous rev-list walk options'

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

test_expect_success 'setup graph with merge' '
	grit init repo &&
	cd repo &&
	base=$(doit 1 A) &&
	c_tip=$(doit 2 C "$base") &&
	d_tip=$(doit 3 D "$base") &&
	m_tip=$(doit 4 M "$d_tip" "$c_tip") &&
	e_tip=$(doit 5 E "$m_tip") &&
	git update-ref refs/tags/base "$base" &&
	git update-ref refs/tags/c "$c_tip" &&
	git update-ref refs/tags/d "$d_tip" &&
	git update-ref refs/tags/e "$e_tip" &&
	git update-ref refs/heads/master "$e_tip"
'

test_expect_success '--first-parent ignores merged side commits' '
	cd repo &&
	c_tip=$(git rev-parse c) &&
	git rev-list base..e >all &&
	git rev-list --first-parent base..e >fp &&
	grep -q "$c_tip" all &&
	! grep -q "$c_tip" fp
'

test_expect_success '--ancestry-path=<commit> limits to that path' '
	cd repo &&
	c_tip=$(git rev-parse c) &&
	d_tip=$(git rev-parse d) &&
	e_tip=$(git rev-parse e) &&
	git rev-list --ancestry-path=d base..e >actual &&
	grep -q "$d_tip" actual &&
	grep -q "$e_tip" actual &&
	! grep -q "$c_tip" actual
'

test_expect_success '--simplify-by-decoration keeps tagged commits in walk' '
	cd repo &&
	c_tip=$(git rev-parse c) &&
	git update-ref refs/tags/keep-c "$c_tip" &&
	git rev-list base..e >full &&
	git rev-list --simplify-by-decoration base..e >simple &&
	grep -q "$c_tip" simple &&
	lines_full=$(wc -l <full | tr -d " ") &&
	lines_simple=$(wc -l <simple | tr -d " ") &&
	test "$lines_simple" -lt "$lines_full"
'

test_expect_success 'rev-list A..B and rev-list ^A B are the same' '
	cd repo &&
	git rev-list ^base e >expect &&
	git rev-list base..e >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-list --count' '
	cd repo &&
	count=$(git rev-list --count master) &&
	git rev-list master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$count" = "$lines"
'

# --- New tests ---

test_expect_success '--max-count limits output' '
	cd repo &&
	git rev-list --max-count=2 master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "2"
'

test_expect_success '--max-count=0 gives empty output' '
	cd repo &&
	git rev-list --max-count=0 master >actual &&
	test_must_be_empty actual
'

test_expect_success '--skip=1 removes first entry' '
	cd repo &&
	git rev-list master >full &&
	git rev-list --skip=1 master >skipped &&
	tail -n +2 full >expect &&
	test_cmp expect skipped
'

test_expect_success '--skip and --max-count combined' '
	cd repo &&
	git rev-list --skip=1 --max-count=2 master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "2"
'

test_expect_success '--reverse reverses output' '
	cd repo &&
	git rev-list master >forward &&
	git rev-list --reverse master >reversed &&
	awk "{ lines[NR] = \$0 } END { for (i = NR; i >= 1; i--) print lines[i] }" forward >expect &&
	test_cmp expect reversed
'

test_expect_success '--count with range' '
	cd repo &&
	count=$(git rev-list --count base..e) &&
	git rev-list base..e >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$count" = "$lines"
'

test_expect_success '--first-parent with --count' '
	cd repo &&
	count=$(git rev-list --first-parent --count master) &&
	git rev-list --first-parent master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$count" = "$lines"
'

test_expect_success '--topo-order lists all commits' '
	cd repo &&
	git rev-list master >default_order &&
	git rev-list --topo-order master >topo_order &&
	sort default_order >sorted1 &&
	sort topo_order >sorted2 &&
	test_cmp sorted1 sorted2
'

test_expect_success '--date-order lists all commits' '
	cd repo &&
	git rev-list master >default_order &&
	git rev-list --date-order master >date_order &&
	sort default_order >sorted1 &&
	sort date_order >sorted2 &&
	test_cmp sorted1 sorted2
'

test_expect_success '--quiet produces no output' '
	cd repo &&
	git rev-list --quiet master >actual &&
	test_must_be_empty actual
'

test_expect_success '--first-parent reduces count vs full walk' '
	cd repo &&
	git rev-list master >full &&
	git rev-list --first-parent master >fp &&
	lines_full=$(wc -l <full | tr -d " ") &&
	lines_fp=$(wc -l <fp | tr -d " ") &&
	test "$lines_fp" -le "$lines_full"
'

test_expect_success '--parents shows parent hashes' '
	cd repo &&
	git rev-list --parents --max-count=1 master >actual &&
	# should have at least 2 hashes (commit + parent)
	words=$(wc -w <actual | tr -d " ") &&
	test "$words" -ge 2
'

test_expect_success 'range A..B excludes A and ancestors' '
	cd repo &&
	base_oid=$(git rev-parse base) &&
	git rev-list base..e >actual &&
	! grep -q "$base_oid" actual
'

test_expect_success '^A B same as A..B' '
	cd repo &&
	git rev-list base..e >range &&
	git rev-list ^base e >caret &&
	test_cmp range caret
'

test_expect_success '--ancestry-path filters to path descendants' '
	cd repo &&
	d_tip=$(git rev-parse d) &&
	git rev-list --ancestry-path=d base..e >actual &&
	grep -q "$d_tip" actual
'

test_expect_success '--max-count=-1 returns all' '
	cd repo &&
	git rev-list master >all &&
	git rev-list --max-count=-1 master >neg_one &&
	test_cmp all neg_one
'

test_done

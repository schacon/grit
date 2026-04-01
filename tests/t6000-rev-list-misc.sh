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

test_done

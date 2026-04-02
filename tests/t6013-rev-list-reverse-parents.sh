#!/bin/sh
# Test --reverse combined with --parents for rev-list.

test_description='rev-list --reverse --parents'

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
#   root -- A -- B -- M (merge B + C) -- D
#            \-- C --/
test_expect_success 'setup merge topology' '
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

test_expect_success '--reverse reverses output order' '
	cd repo &&
	git rev-list HEAD >forward &&
	git rev-list --reverse HEAD >reversed &&
	tac forward >expected &&
	test_cmp expected reversed
'

test_expect_success '--parents shows parent hashes' '
	cd repo &&
	m=$(cat ../oid_m) &&
	b=$(cat ../oid_b) &&
	c=$(cat ../oid_c) &&
	git rev-list --parents HEAD >actual &&
	grep "$m $b $c" actual
'

test_expect_success '--parents root commit has no parents listed' '
	cd repo &&
	root=$(cat ../oid_root) &&
	git rev-list --parents HEAD >actual &&
	grep "^${root}$" actual
'

test_expect_success '--reverse --parents first line is root with no parents' '
	cd repo &&
	root=$(cat ../oid_root) &&
	git rev-list --reverse --parents HEAD >actual &&
	head -1 actual >first_line &&
	echo "$root" >expect &&
	test_cmp expect first_line
'

test_expect_success '--reverse --parents last line is tip commit' '
	cd repo &&
	d=$(cat ../oid_d) &&
	m=$(cat ../oid_m) &&
	git rev-list --reverse --parents HEAD >actual &&
	tail -1 actual >last_line &&
	echo "$d $m" >expect &&
	test_cmp expect last_line
'

test_expect_success '--reverse --parents merge line includes both parents' '
	cd repo &&
	m=$(cat ../oid_m) &&
	b=$(cat ../oid_b) &&
	c=$(cat ../oid_c) &&
	git rev-list --reverse --parents HEAD >actual &&
	grep "$m $b $c" actual
'

test_expect_success '--reverse --parents --count' '
	cd repo &&
	git rev-list --reverse --count HEAD >actual &&
	echo 6 >expect &&
	test_cmp expect actual
'

test_expect_success '--reverse preserves parent info across range' '
	cd repo &&
	a=$(cat ../oid_a) &&
	d=$(cat ../oid_d) &&
	git rev-list --reverse --parents "$a"..master >actual &&
	head -1 actual >first &&
	# First commits after A should list A as parent
	grep "$a" first
'

test_done

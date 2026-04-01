#!/bin/sh
# Ported subset from git/t/t6006-rev-list-format.sh.

test_description='git rev-list format output'

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

test_expect_success 'setup repository with two commits' '
	grit init repo &&
	cd repo &&
	head1=$(doit 1 "added foo") &&
	head2=$(doit 2 "changed foo" "$head1") &&
	git update-ref refs/heads/master "$head2" &&
	echo "$head1" >head1 &&
	echo "$head2" >head2
'

test_expect_success '--format=%s includes commit headers' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format supports %H and %h' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short=4 "$head2") &&
	cat >expect <<-EOF &&
	commit $head2
	$head2 $short2
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="%H %h" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--quiet suppresses output' '
	cd repo &&
	git rev-list --quiet refs/heads/master >actual &&
	test_path_is_file actual &&
	lines=$(wc -c <actual | tr -d " ") &&
	test "$lines" = "0"
'

test_expect_success 'percent literal %%' '
	cd repo &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	%h
	EOF
	git rev-list --max-count=1 --format="%%h" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H alone' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	$head2
	commit $head1
	$head1
	EOF
	git rev-list --format=%H refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %h with default abbreviation' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short "$head2") &&
	git rev-list --max-count=1 --format="%h" refs/heads/master >actual &&
	# Extract the formatted line (second line)
	sed -n 2p actual >hash_line &&
	echo "$short2" >expect &&
	test_cmp expect hash_line
'

test_expect_success '--format with multiple specifiers' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short=4 "$head2") &&
	cat >expect <<-EOF &&
	commit $head2
	hash=$head2 short=$short2 subject=changed foo
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="hash=%H short=%h subject=%s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with literal text only' '
	cd repo &&
	head2=$(cat head2) &&
	head1=$(cat head1) &&
	cat >expect <<-EOF &&
	commit $head2
	hello world
	commit $head1
	hello world
	EOF
	git rev-list --format="hello world" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'setup third commit' '
	cd repo &&
	head2=$(cat head2) &&
	head3=$(doit 3 "third commit" "$head2") &&
	git update-ref refs/heads/master "$head3" &&
	echo "$head3" >head3
'

test_expect_success '--format %s with three commits' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	third commit
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--max-count=1 with --format shows only one' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	third commit
	EOF
	git rev-list --max-count=1 --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with empty string' '
	cd repo &&
	head3=$(cat head3) &&
	head2=$(cat head2) &&
	head1=$(cat head1) &&
	cat >expect <<-EOF &&
	commit $head3

	commit $head2

	commit $head1

	EOF
	git rev-list --format="" refs/heads/master >actual &&
	test_cmp expect actual
'

test_done

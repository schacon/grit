#!/bin/sh
# Ported from git/t/t1010-mktree.sh

test_description='grit mktree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	grit init repo &&
	cd repo &&
	grit config user.email "author@example.com" &&
	grit config user.name "A U Thor" &&
	for d in a a- a0
	do
		mkdir "$d" && echo "$d/one" >"$d/one" &&
		grit add "$d" || return 1
	done &&
	echo one >one &&
	grit add one &&
	grit write-tree >tree &&
	grit ls-tree $(cat tree) >top &&
	grit ls-tree -r $(cat tree) >all &&
	test_tick &&
	grit commit -q -m one &&
	H=$(grit rev-parse HEAD) &&
	grit update-index --add --cacheinfo 160000,$H,sub &&
	test_tick &&
	grit commit -q -m two &&
	grit rev-parse HEAD^{tree} >tree.withsub &&
	grit ls-tree HEAD >top.withsub &&
	grit ls-tree -r HEAD >all.withsub
'

test_expect_success 'ls-tree piped to mktree (1)' '
	cd repo &&
	grit mktree <top >actual &&
	test_cmp tree actual
'

test_expect_success 'ls-tree piped to mktree (2)' '
	cd repo &&
	grit mktree <top.withsub >actual &&
	test_cmp tree.withsub actual
'

test_expect_success 'ls-tree output in wrong order given to mktree (1)' '
	cd repo &&
	sort -r <top |
	grit mktree >actual &&
	test_cmp tree actual
'

test_expect_success 'ls-tree output in wrong order given to mktree (2)' '
	cd repo &&
	sort -r <top.withsub |
	grit mktree >actual &&
	test_cmp tree.withsub actual
'

test_expect_success 'mktree refuses to read ls-tree -r output (1)' '
	cd repo &&
	test_must_fail grit mktree <all
'

test_expect_success 'mktree refuses to read ls-tree -r output (2)' '
	cd repo &&
	test_must_fail grit mktree <all.withsub
'

test_done

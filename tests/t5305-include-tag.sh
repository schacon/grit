#!/bin/sh

test_description='git pack-object tag handling'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo c >d &&
	git update-index --add d &&
	tree=$(git write-tree) &&
	commit=$(git commit-tree $tree </dev/null) &&
	echo "object $commit" >sig &&
	echo "type commit" >>sig &&
	echo "tag mytag" >>sig &&
	echo "tagger $(git var GIT_COMMITTER_IDENT)" >>sig &&
	echo >>sig &&
	echo "our test tag" >>sig &&
	tag=$(git mktag <sig) &&
	rm d sig &&
	git update-ref refs/tags/mytag $tag
'

test_expect_success 'pack with --revs includes reachable objects' '
	echo refs/tags/mytag |
	git pack-objects --revs testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows tag object' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "tag" output &&
	test_grep "commit" output
'

test_expect_success 'pack --all includes all objects' '
	git pack-objects --all allpack &&
	git verify-pack -v allpack-*.pack >output &&
	test_grep "blob" output &&
	test_grep "tree" output &&
	test_grep "commit" output &&
	test_grep "tag" output
'

test_done

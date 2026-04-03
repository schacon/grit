#!/bin/sh

test_description='tests for git clone --revision'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "Hello" >README &&
	git add README &&
	git commit -m "initial commit" &&
	echo "Hello world" >README &&
	git add README &&
	git commit -m "second commit" &&
	git tag -a -m "v1.0" v1.0 &&
	echo "Hello world!" >README &&
	git add README &&
	git commit -m "third commit" &&
	git switch -c feature v1.0 &&
	echo "Hello world!" >README &&
	git add README &&
	git commit -m "feature commit" &&
	git switch main
'

test_expect_success 'clone with --revision being a branch' '
	test_when_finished "rm -rf dst" &&
	git clone --revision=refs/heads/feature . dst &&
	git rev-parse refs/heads/feature >expect &&
	git -C dst rev-parse HEAD >actual &&
	test_must_fail git -C dst symbolic-ref -q HEAD >/dev/null &&
	test_cmp expect actual
'

test_expect_success 'clone with --revision being a tag' '
	test_when_finished "rm -rf dst" &&
	git clone --revision=refs/tags/v1.0 . dst &&
	git rev-parse refs/tags/v1.0^{} >expect &&
	git -C dst rev-parse HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'clone with --revision being HEAD' '
	test_when_finished "rm -rf dst" &&
	git clone --revision=HEAD . dst &&
	git rev-parse HEAD >expect &&
	git -C dst rev-parse HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'clone with --revision and --branch fails' '
	test_when_finished "rm -rf dst" &&
	test_must_fail git clone --revision=refs/heads/main --branch=main . dst
'

test_expect_success 'clone with --revision and --mirror fails' '
	test_when_finished "rm -rf dst" &&
	test_must_fail git clone --revision=refs/heads/main --mirror . dst
'

test_done

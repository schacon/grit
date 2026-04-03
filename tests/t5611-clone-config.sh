#!/bin/sh

test_description='tests for git clone -c key=value'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo content >file &&
	git add file &&
	git commit -m one
'

test_expect_failure 'clone -c sets config in cloned repo' '
	rm -rf child &&
	git clone -c core.foo=bar . child &&
	echo bar >expect &&
	git --git-dir=child/.git config core.foo >actual &&
	test_cmp expect actual
'

test_expect_failure 'clone -c can set multi-keys' '
	rm -rf child &&
	git clone -c core.foo=bar -c core.foo=baz . child &&
	test_write_lines bar baz >expect &&
	git --git-dir=child/.git config --get-all core.foo >actual &&
	test_cmp expect actual
'

test_expect_failure 'clone -c without a value is boolean true' '
	rm -rf child &&
	git clone -c core.foo . child &&
	echo true >expect &&
	git --git-dir=child/.git config --bool core.foo >actual &&
	test_cmp expect actual
'

test_expect_success 'basic clone works' '
	rm -rf child &&
	git clone . child &&
	test_path_is_dir child/.git
'

test_done

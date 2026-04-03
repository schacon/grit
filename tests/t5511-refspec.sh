#!/bin/sh

test_description='refspec parsing'

. ./test-lib.sh

# grit does not support ls-remote with named remotes configured via
# git config, so we test refspec-like config parsing indirectly.

test_expect_success 'setup' '
	git init
'

test_expect_success 'can set remote.frotz.url via config' '
	git config remote.frotz.url "." &&
	test "$(git config remote.frotz.url)" = "."
'

test_expect_success 'can set push refspec' '
	git config remote.frotz.push "refs/heads/*:refs/remotes/frotz/*" &&
	test "$(git config remote.frotz.push)" = "refs/heads/*:refs/remotes/frotz/*"
'

test_expect_success 'can set fetch refspec' '
	git config remote.frotz.fetch "+refs/heads/*:refs/remotes/frotz/*" &&
	test "$(git config remote.frotz.fetch)" = "+refs/heads/*:refs/remotes/frotz/*"
'

test_expect_success 'can remove remote section' '
	git config --remove-section remote.frotz &&
	test_must_fail git config remote.frotz.url
'

test_expect_failure 'ls-remote with named remote' '
	git config remote.frotz.url "." &&
	git config remote.frotz.fetch "+refs/heads/*:refs/remotes/frotz/*" &&
	git ls-remote frotz
'

test_done

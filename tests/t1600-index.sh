#!/bin/sh

test_description='index file specific tests'

. ./test-lib.sh

sane_unset GIT_TEST_SPLIT_INDEX

test_expect_success 'setup' '
	git init &&
	echo 1 >a
'

test_expect_success 'bogus GIT_INDEX_VERSION issues warning' '
	(
		rm -f .git/index &&
		GIT_INDEX_VERSION=2bogus &&
		export GIT_INDEX_VERSION &&
		git add a 2>err &&
		sed "s/[0-9]//" err >actual.err &&
		sed -e "s/ Z$/ /" <<-\EOF >expect.err &&
			warning: GIT_INDEX_VERSION set, but the value is invalid.
			Using version Z
		EOF
		test_cmp expect.err actual.err
	)
'

test_expect_success 'out of bounds GIT_INDEX_VERSION issues warning' '
	(
		rm -f .git/index &&
		GIT_INDEX_VERSION=1 &&
		export GIT_INDEX_VERSION &&
		git add a 2>err &&
		sed "s/[0-9]//" err >actual.err &&
		sed -e "s/ Z$/ /" <<-\EOF >expect.err &&
			warning: GIT_INDEX_VERSION set, but the value is invalid.
			Using version Z
		EOF
		test_cmp expect.err actual.err
	)
'

test_expect_success 'basic index add and read' '
	rm -f .git/index &&
	git add a &&
	git ls-files --stage >actual &&
	grep "a$" actual
'

test_expect_success 'index survives round-trip' '
	git add a &&
	git commit -m "add a" &&
	git ls-files --stage >before &&
	git read-tree HEAD &&
	git ls-files --stage >after &&
	# Both should show the file
	grep "a" before &&
	grep "a" after
'

test_done

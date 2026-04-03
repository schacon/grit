#!/bin/sh

test_description='test finding specific blobs in the revision walking'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	git commit --allow-empty -m "empty initial commit" &&

	echo "Hello, world!" >greeting &&
	git add greeting &&
	git commit -m "add the greeting blob" &&

	echo asdf >unrelated &&
	git add unrelated &&
	git commit -m "unrelated history"
'

test_expect_failure 'find the greeting blob (not implemented)' '
	git log --format=%s --find-object=HEAD~1:greeting >actual &&
	cat >expect <<-EOF &&
	add the greeting blob
	EOF
	test_cmp expect actual
'

test_expect_success 'log --format=%s works' '
	git log --format=%s >actual &&
	grep "unrelated history" actual &&
	grep "add the greeting blob" actual &&
	grep "empty initial commit" actual
'

test_done

#!/bin/sh

test_description='Test reffiles backend consistency check'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	git commit --allow-empty -m initial &&
	git checkout -b default-branch &&
	git tag default-tag
'

test_expect_success 'refs verify runs without error on clean repo' '
	git refs verify 2>err &&
	! grep -i error err
'

test_expect_success 'ref name should be checked for invalid names' '
	cp .git/refs/heads/default-branch ".git/refs/heads/.starts-with-dot" &&
	test_must_fail git refs verify 2>err &&
	grep "badRefName" err &&
	rm ".git/refs/heads/.starts-with-dot"
'

test_expect_success 'ref name check adapted into fsck messages' '
	cp .git/refs/heads/default-branch ".git/refs/heads/.branch-bad" &&
	git -c fsck.badRefName=warn refs verify 2>err &&
	grep "warning.*badRefName" err &&
	rm ".git/refs/heads/.branch-bad"
'

test_done

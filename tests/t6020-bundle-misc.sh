#!/bin/sh

test_description='Test git-bundle'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo one >file && git add file &&
	test_tick && git commit -m one &&
	echo two >file && git add file &&
	test_tick && git commit -m two &&
	echo three >file && git add file &&
	test_tick && git commit -m three &&
	git tag v1.0
'

test_expect_success 'bundle create with HEAD' '
	cd repo &&
	git bundle create ../test.bundle HEAD
'

test_expect_success 'bundle verify' '
	cd repo &&
	git bundle verify ../test.bundle
'

test_expect_success 'bundle list-heads' '
	cd repo &&
	git bundle list-heads ../test.bundle >output &&
	grep HEAD output
'

test_expect_success 'bundle create with branch ref' '
	cd repo &&
	git bundle create ../branch.bundle main
'

test_expect_success 'bundle verify branch bundle' '
	cd repo &&
	git bundle verify ../branch.bundle
'

test_expect_success 'bundle list-heads shows branch' '
	cd repo &&
	git bundle list-heads ../branch.bundle >output &&
	grep "main" output
'

test_expect_success 'unbundle extracts objects' '
	cd repo &&
	git bundle unbundle ../test.bundle >unbundle-output 2>&1
'

test_expect_success 'bundle create with tag' '
	cd repo &&
	git bundle create ../tagged.bundle v1.0
'

test_expect_success 'bundle verify tagged' '
	cd repo &&
	git bundle verify ../tagged.bundle
'

test_expect_success 'bundle list-heads tagged shows tag' '
	cd repo &&
	git bundle list-heads ../tagged.bundle >output &&
	grep "v1.0" output
'

test_expect_success 'create full bundle and verify object count' '
	cd repo &&
	git bundle create ../full.bundle --all &&
	git bundle verify ../full.bundle
'

test_done

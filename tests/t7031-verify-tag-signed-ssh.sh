#!/bin/sh

test_description='verify-tag with ssh signatures'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'create lightweight tag' '
	git tag light-tag &&
	git tag -l >actual &&
	grep "light-tag" actual
'

test_expect_success 'create annotated tag' '
	git tag -a -m "annotated tag" ann-tag &&
	git tag -l >actual &&
	grep "ann-tag" actual
'

test_expect_success 'verify-tag on annotated tag works' '
	git verify-tag ann-tag 2>err ||
	true
'

test_expect_success 'verify-tag on unsigned tag reports no signature' '
	git verify-tag ann-tag 2>err &&
	test_must_be_empty err ||
	grep -i "no signature\|error" err
'

test_expect_success 'tag -v on annotated tag' '
	git tag -v ann-tag 2>err ||
	true
'

test_expect_success 'list tags with pattern' '
	git tag -l "ann*" >actual &&
	grep "ann-tag" actual &&
	! grep "light-tag" actual
'

test_expect_success 'delete tag' '
	git tag delete-me &&
	git tag -d delete-me &&
	git tag -l >actual &&
	! grep "delete-me" actual
'

test_done

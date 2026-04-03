#!/bin/sh

test_description='git update-index --assume-unchanged test'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'setup' '
	: >file &&
	git add file &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	git commit -m initial &&
	git branch other &&
	echo upstream >file &&
	git add file &&
	git commit -m upstream
'

test_expect_success 'assume-unchanged flag can be set and cleared' '
	git update-index --assume-unchanged file &&
	git update-index --no-assume-unchanged file
'

test_done

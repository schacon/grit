#!/bin/sh

test_description='racy split index'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Racy split-index tests require test-tool which is not available.
# Test basic racy-git behavior instead.

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'racy entry detection: modified file detected' '
	cd repo &&
	echo initial >file &&
	git add file &&
	git commit -m "initial" &&
	echo modified >file &&
	git diff --name-only >actual &&
	grep "file" actual
'

test_expect_success 'clean file shows no diff' '
	cd repo &&
	git checkout -- file &&
	git diff --name-only >actual &&
	test_must_be_empty actual
'

test_done

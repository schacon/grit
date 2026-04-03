#!/bin/sh
# Ported from upstream git t7514-commit-patch.sh

test_description='commit with -p (patch mode)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init commit-patch &&
	cd commit-patch &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo line1 >file &&
	git add file &&
	test_tick &&
	git commit -m commit1
'

test_expect_success 'commit -a works' '
	cd commit-patch &&
	echo more >>file &&
	test_tick &&
	git commit -a -m commit2 &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'diff after commit is empty' '
	cd commit-patch &&
	git diff >actual &&
	test_must_be_empty actual
'

test_done

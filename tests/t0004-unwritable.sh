#!/bin/sh
#
# Ported from git/t/t0004-unwritable.sh

test_description='detect unwritable repository and fail correctly'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	>file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	echo >file &&
	git add file
'

test_expect_success POSIXPERM 'write-tree should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git write-tree
'

test_expect_success POSIXPERM 'commit should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git commit -m second
'

test_expect_success POSIXPERM 'update-index should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	echo 6O >file &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git update-index file
'

test_expect_success POSIXPERM 'add should notice unwritable repository' '
	test_when_finished "chmod 775 .git/objects .git/objects/??" &&
	echo b >file &&
	chmod a-w .git/objects .git/objects/?? &&
	test_must_fail git add file
'

test_done

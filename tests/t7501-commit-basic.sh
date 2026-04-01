#!/bin/sh
# Ported from git/t/t7501-commit-basic-functionality.sh
# Tests for 'grit commit'.

test_description='grit commit basic functionality'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com"
'

test_expect_success 'initial commit' '
	cd repo &&
	echo "hello" >file.txt &&
	git add file.txt &&
	git commit -m "initial commit" 2>stderr &&
	grep "root-commit" stderr &&
	git cat-file -t HEAD >type &&
	echo "commit" >expected &&
	test_cmp expected type
'

test_expect_success 'commit message is stored correctly' '
	cd repo &&
	git cat-file -p HEAD >actual &&
	grep "initial commit" actual
'

test_expect_success 'second commit has parent' '
	cd repo &&
	echo "world" >>file.txt &&
	git add file.txt &&
	git commit -m "second commit" 2>stderr &&
	! grep "root-commit" stderr &&
	git cat-file -p HEAD >actual &&
	grep "^parent " actual
'

test_expect_success 'commit -m with multiple messages' '
	cd repo &&
	echo "more" >>file.txt &&
	git add file.txt &&
	git commit -m "first paragraph" -m "second paragraph" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "first paragraph" actual &&
	grep "second paragraph" actual
'

test_expect_success 'commit -a stages tracked files' '
	cd repo &&
	echo "auto-staged" >>file.txt &&
	git commit -a -m "auto staged commit" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "auto staged commit" actual
'

test_expect_success 'commit -F reads message from file' '
	cd repo &&
	echo "new content" >>file.txt &&
	git add file.txt &&
	echo "message from file" >msg.txt &&
	git commit -F msg.txt 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "message from file" actual
'

test_expect_success 'commit without changes fails (no --allow-empty)' '
	cd repo &&
	! git commit -m "empty" 2>/dev/null
'

test_expect_success 'commit --allow-empty succeeds' '
	cd repo &&
	git commit --allow-empty -m "empty commit" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "empty commit" actual
'

test_expect_success 'commit --quiet suppresses output' '
	cd repo &&
	echo "quiet" >>file.txt &&
	git add file.txt &&
	git commit -q -m "quiet commit" 2>stderr &&
	test ! -s stderr
'

test_expect_success 'commit respects GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL' '
	cd repo &&
	echo "env author" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Custom Author" GIT_AUTHOR_EMAIL="custom@test.com" \
		git commit -m "custom author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "Custom Author <custom@test.com>" actual
'

test_expect_success 'commit --author overrides identity' '
	cd repo &&
	echo "override" >>file.txt &&
	git add file.txt &&
	git commit --author="Override Author <override@test.com>" -m "override author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "Override Author <override@test.com>" actual
'

test_done

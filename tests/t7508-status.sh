#!/bin/sh
# Tests for 'gust status'.

test_description='gust status'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "init" >file.txt &&
	git add file.txt &&
	git commit -m "initial" 2>/dev/null
'

test_expect_success 'clean status' '
	cd repo &&
	git status >../actual &&
	grep "nothing to commit" ../actual
'

test_expect_success 'status shows branch' '
	cd repo &&
	git status >../actual &&
	grep "On branch master" ../actual
'

test_expect_success 'modified file shows as unstaged' '
	cd repo &&
	echo "changed" >>file.txt &&
	git status >../actual &&
	grep "modified:.*file.txt" ../actual &&
	grep "Changes not staged for commit" ../actual
'

test_expect_success 'staged file shows as staged' '
	cd repo &&
	git add file.txt &&
	git status >../actual &&
	grep "Changes to be committed" ../actual
'

test_expect_success 'untracked file shows' '
	cd repo &&
	echo "new" >untracked.txt &&
	git status >../actual &&
	grep "Untracked files" ../actual &&
	grep "untracked.txt" ../actual
'

test_expect_success 'short format shows XY codes' '
	cd repo &&
	git status -s >../actual &&
	grep "^M " ../actual &&
	grep "^??" ../actual
'

test_expect_success 'porcelain format shows branch header' '
	cd repo &&
	git status --porcelain -b >../actual &&
	grep "^## master" ../actual
'

test_expect_success 'deleted file shows as deleted' '
	cd repo &&
	git commit -m "commit staged" 2>/dev/null &&
	rm file.txt &&
	git status -s >../actual &&
	grep "^ D file.txt" ../actual
'

test_done

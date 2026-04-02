#!/bin/sh
# Test .git file support (gitdir: pointer)

test_description='grit .git file (gitdir: pointer) support'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup real repo' '
	grit init real-repo &&
	cd real-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "content" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial"
'

test_expect_success 'create linked worktree via .git file' '
	mkdir linked &&
	echo "gitdir: $TRASH_DIRECTORY/real-repo/.git" >linked/.git &&
	test -f linked/.git
'

test_expect_success 'rev-parse --git-dir works through .git file' '
	cd linked &&
	grit rev-parse --git-dir >actual 2>&1 &&
	# Should resolve to the real .git directory
	test -s actual
'

test_expect_success 'status works through .git file' '
	cd linked &&
	grit status >output 2>&1 &&
	# Should show master branch
	grep -i "master\|branch" output
'

test_expect_success 'log works through .git file' '
	cd linked &&
	grit log --oneline >output 2>&1 &&
	grep "initial" output
'

test_expect_success 'rev-parse HEAD works through .git file' '
	cd linked &&
	grit rev-parse HEAD >actual &&
	cd $TRASH_DIRECTORY/real-repo &&
	grit rev-parse HEAD >expect &&
	test_cmp expect $TRASH_DIRECTORY/linked/actual
'

test_expect_success 'branch list works through .git file' '
	cd linked &&
	grit branch >output &&
	grep "master" output
'

test_expect_success 'cat-file works through .git file' '
	cd linked &&
	grit cat-file -p HEAD >output &&
	grep "initial" output
'

test_expect_success 'ls-files works through .git file' '
	cd linked &&
	grit ls-files >actual &&
	echo "file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse --show-toplevel works through .git file' '
	cd linked &&
	grit rev-parse --show-toplevel >actual 2>&1 &&
	test -s actual
'

test_done

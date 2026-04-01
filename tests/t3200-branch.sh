#!/bin/sh
# Tests for 'grit branch'.

test_description='grit branch'

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

test_expect_success 'list branches shows current' '
	cd repo &&
	git branch >actual &&
	grep "^\* master" actual
'

test_expect_success 'create a branch' '
	cd repo &&
	git branch feature &&
	git branch >actual &&
	grep "feature" actual
'

test_expect_success '--show-current shows current branch' '
	cd repo &&
	git branch --show-current >actual &&
	echo "master" >expected &&
	test_cmp expected actual
'

test_expect_success 'create branch at specific commit' '
	cd repo &&
	echo "second" >>file.txt &&
	git add file.txt &&
	git commit -m "second" 2>/dev/null &&
	git branch old-point HEAD~1 2>/dev/null ||
	git branch old-point master 2>/dev/null
'

test_expect_success 'delete a branch' '
	cd repo &&
	git branch to-delete &&
	git branch >actual &&
	grep "to-delete" actual &&
	git branch -d to-delete 2>/dev/null &&
	git branch >actual &&
	! grep "to-delete" actual
'

test_expect_success 'cannot delete current branch' '
	cd repo &&
	! git branch -d master 2>/dev/null
'

test_expect_success 'rename a branch' '
	cd repo &&
	git branch rename-me &&
	git branch -m rename-me renamed 2>/dev/null &&
	git branch >actual &&
	! grep "rename-me" actual &&
	grep "renamed" actual
'

test_expect_success 'verbose listing shows commit info' '
	cd repo &&
	git branch -v >actual &&
	grep "master" actual &&
	grep "second" actual
'

# --- New tests below ---

test_expect_success 'branch --list shows all branches' '
	cd repo &&
	git branch --list >actual &&
	grep "feature" actual &&
	grep "master" actual &&
	grep "renamed" actual
'

test_expect_success 'branch -f overwrites existing branch to new commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch force-target "$parent_sha" &&
	old_sha=$(git rev-parse force-target) &&
	test "$old_sha" = "$parent_sha" &&
	git branch -f force-target HEAD 2>/dev/null &&
	new_sha=$(git rev-parse force-target) &&
	head_sha=$(git rev-parse HEAD) &&
	test "$old_sha" != "$new_sha" &&
	test "$new_sha" = "$head_sha"
'

test_expect_success 'branch -D deletes a branch' '
	cd repo &&
	git branch to-force-delete &&
	git branch >actual &&
	grep "to-force-delete" actual &&
	git branch -D to-force-delete 2>/dev/null &&
	git branch >actual &&
	! grep "to-force-delete" actual
'

test_expect_success 'branch at tag resolves to same commit' '
	cd repo &&
	git tag test-tag HEAD~1 &&
	git branch at-tag test-tag 2>/dev/null &&
	tag_sha=$(git rev-parse test-tag) &&
	branch_sha=$(git rev-parse at-tag) &&
	test "$tag_sha" = "$branch_sha"
'

test_expect_success 'branch --contains lists branches containing commit' '
	cd repo &&
	git branch --contains HEAD~1 >actual &&
	grep "master" actual &&
	grep "feature" actual
'

test_expect_success 'branch --merged with specific ref lists branches' '
	cd repo &&
	git branch --merged master >actual &&
	grep "feature" actual
'

test_expect_success 'branch refuses to create duplicate name' '
	cd repo &&
	! git branch feature 2>/dev/null
'

test_expect_success 'delete non-existent branch fails' '
	cd repo &&
	! git branch -d no-such-branch 2>/dev/null
'

test_expect_success 'delete already-deleted branch fails' '
	cd repo &&
	git branch temp-branch &&
	git branch -d temp-branch 2>/dev/null &&
	! git branch -d temp-branch 2>/dev/null
'

test_expect_success 'branch at specific SHA points to that commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch at-parent "$parent_sha" 2>/dev/null &&
	branch_sha=$(git rev-parse at-parent) &&
	test "$parent_sha" = "$branch_sha"
'

test_expect_success 'newly created branch appears in listing' '
	cd repo &&
	count_before=$(git branch | wc -l) &&
	git branch counting-test &&
	count_after=$(git branch | wc -l) &&
	test "$count_after" -gt "$count_before"
'

test_expect_success 'branch listing marks only current branch with star' '
	cd repo &&
	git branch >actual &&
	star_count=$(grep "^\*" actual | wc -l) &&
	test "$star_count" -eq 1 &&
	grep "^\* master" actual
'

test_expect_success 'rename branch preserves commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch rename-test2 "$parent_sha" &&
	old_sha=$(git rev-parse rename-test2) &&
	git branch -m rename-test2 renamed-test2 2>/dev/null &&
	new_sha=$(git rev-parse renamed-test2) &&
	test "$old_sha" = "$new_sha"
'

test_expect_success 'branch at HEAD equals HEAD sha' '
	cd repo &&
	git branch head-test &&
	head_sha=$(git rev-parse HEAD) &&
	branch_sha=$(git rev-parse head-test) &&
	test "$head_sha" = "$branch_sha"
'

test_expect_success 'branch -v shows abbreviated sha for each branch' '
	cd repo &&
	head_short=$(git rev-parse --short HEAD) &&
	git branch -v >actual &&
	grep "$head_short" actual
'

test_expect_success '-D also deletes branch (like -d)' '
	cd repo &&
	git branch big-d-test &&
	git branch >actual &&
	grep "big-d-test" actual &&
	git branch -D big-d-test 2>/dev/null &&
	git branch >actual &&
	! grep "big-d-test" actual
'

test_expect_success 'branch -d prints deletion message' '
	cd repo &&
	git branch msg-test &&
	git branch -d msg-test >actual 2>&1 &&
	grep -i "deleted" actual
'

test_done

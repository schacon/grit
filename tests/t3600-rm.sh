#!/bin/sh
# Ported from git/t/t3600-rm.sh
# Tests for 'grit rm'.

test_description='grit rm'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com"
'

test_expect_success 'setup initial files' '
	cd repo &&
	echo foo >foo &&
	echo bar >bar &&
	echo baz >baz &&
	git add foo bar baz &&
	git commit -m "add normal files"
'

test_expect_success 'pre-check: foo is in index' '
	cd repo &&
	git ls-files --error-unmatch foo
'

test_expect_success 'git rm --cached foo removes foo from index only' '
	cd repo &&
	git rm --cached foo &&
	test_path_is_file foo &&
	test_must_fail git ls-files --error-unmatch foo
'

test_expect_success 'git rm --cached foo succeeds if index matches file' '
	cd repo &&
	echo content >foo &&
	git add foo &&
	git rm --cached foo &&
	test_path_is_file foo
'

test_expect_success 'git rm --cached foo succeeds if index matches HEAD but file differs' '
	cd repo &&
	echo content >foo &&
	git add foo &&
	git commit -m "foo content" &&
	echo "other content" >foo &&
	git rm --cached foo &&
	test_path_is_file foo &&
	test_must_fail git ls-files --error-unmatch foo
'

test_expect_success 'git rm --cached foo fails if index matches neither file nor HEAD' '
	cd repo &&
	echo newcontent >foo &&
	git add foo &&
	git commit -m "foo newcontent" &&
	echo "other content" >foo &&
	git add foo &&
	echo "yet another content" >foo &&
	test_must_fail git rm --cached foo
'

test_expect_success 'git rm --cached -f foo works when --cached alone does not' '
	cd repo &&
	git rm --cached -f foo
'

test_expect_success 'post-check: foo exists but not in index' '
	cd repo &&
	test_path_is_file foo &&
	test_must_fail git ls-files --error-unmatch foo
'

test_expect_success 'pre-check: bar is in index' '
	cd repo &&
	git ls-files --error-unmatch bar
'

test_expect_success 'git rm bar removes bar from index and worktree' '
	cd repo &&
	git rm bar
'

test_expect_success 'post-check: bar is gone from index and worktree' '
	cd repo &&
	test_path_is_missing bar &&
	test_must_fail git ls-files --error-unmatch bar
'

test_expect_success '"rm" output line printed' '
	cd repo &&
	echo frotz >test-file &&
	git add test-file &&
	git commit -m "add file for rm test" &&
	git rm test-file >rm-output.raw &&
	grep "^rm " rm-output.raw >rm-output &&
	test_line_count = 1 rm-output
'

test_expect_success '"rm" output suppressed with --quiet' '
	cd repo &&
	echo frotz2 >test-file2 &&
	git add test-file2 &&
	git commit -m "add file2 for rm --quiet test" &&
	git rm --quiet test-file2 >rm-output &&
	test_must_be_empty rm-output
'

test_expect_success 're-add foo and baz' '
	cd repo &&
	git add foo baz &&
	git ls-files --error-unmatch foo baz
'

test_expect_success 'modified foo -- rm should refuse' '
	cd repo &&
	echo modified >>foo &&
	test_must_fail git rm foo baz &&
	test_path_is_file foo &&
	test_path_is_file baz &&
	git ls-files --error-unmatch foo baz
'

test_expect_success 'modified foo -- rm -f should work' '
	cd repo &&
	git rm -f foo baz &&
	test_path_is_missing foo &&
	test_path_is_missing baz &&
	test_must_fail git ls-files --error-unmatch foo &&
	test_must_fail git ls-files --error-unmatch baz
'

test_expect_success 'remove intent-to-add file without --force' '
	cd repo &&
	echo content >intent-to-add &&
	git add -N intent-to-add &&
	git rm --cached intent-to-add &&
	test_must_fail git ls-files --error-unmatch intent-to-add
'

test_expect_success 'recursive test setup' '
	cd repo &&
	mkdir -p frotz &&
	echo qfwfq >frotz/nitfol &&
	git add frotz &&
	git commit -m "subdir test"
'

test_expect_success 'recursive without -r fails' '
	cd repo &&
	test_must_fail git rm frotz &&
	test_path_is_dir frotz &&
	test_path_is_file frotz/nitfol
'

test_expect_success 'recursive with -r -f' '
	cd repo &&
	git rm -f -r frotz &&
	test_path_is_missing frotz/nitfol &&
	test_path_is_missing frotz
'

test_expect_success 'remove nonexistent file returns nonzero' '
	cd repo &&
	test_must_fail git rm nonexistent
'

test_expect_success 'remove nonexistent file with --ignore-unmatch succeeds' '
	cd repo &&
	git rm --ignore-unmatch nonexistent
'

test_done

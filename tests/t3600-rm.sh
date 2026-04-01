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
	touch -- -q &&
	git add -- foo bar baz -q &&
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

test_expect_success 'Test that "git rm -- -q" succeeds (remove a file that looks like an option)' '
	cd repo &&
	git rm -- -q
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

test_expect_success 'Remove nonexistent file with --ignore-unmatch' '
	cd repo &&
	git rm --ignore-unmatch nonexistent
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

test_expect_success 'Re-add foo and baz for HEAD tests' '
	cd repo &&
	echo frotz >foo &&
	echo baz_content >baz &&
	git add foo baz &&
	git commit -m "re-add foo and baz" &&
	echo new_frotz >foo &&
	git add foo baz &&
	git ls-files --error-unmatch foo baz
'

test_expect_success 'foo is different in index from HEAD -- rm should refuse' '
	cd repo &&
	test_must_fail git rm foo baz &&
	test_path_is_file foo &&
	test_path_is_file baz &&
	git ls-files --error-unmatch foo baz
'

test_expect_success 'but with -f it should work' '
	cd repo &&
	git rm -f foo baz &&
	test_path_is_missing foo &&
	test_path_is_missing baz &&
	test_must_fail git ls-files --error-unmatch foo &&
	test_must_fail git ls-files --error-unmatch baz
'

test_expect_success 'refuse to remove cached empty file with modifications' '
	cd repo &&
	>empty &&
	git add empty &&
	echo content >empty &&
	test_must_fail git rm --cached empty
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

test_expect_success 'Recursive with -r but dirty' '
	cd repo &&
	echo qfwfq >>frotz/nitfol &&
	test_must_fail git rm -r frotz &&
	test_path_is_dir frotz &&
	test_path_is_file frotz/nitfol
'

test_expect_success 'recursive with -r -f' '
	cd repo &&
	git rm -f -r frotz &&
	test_path_is_missing frotz/nitfol
'

test_expect_success 'remove nonexistent file returns nonzero' '
	cd repo &&
	test_must_fail git rm nonexistent
'

test_expect_success 'remove nonexistent file with --ignore-unmatch succeeds' '
	cd repo &&
	git rm --ignore-unmatch nonexistent
'

test_expect_success 'rm --dry-run does not remove file or update index' '
	cd repo &&
	echo dryrun >dryrun_file &&
	git add dryrun_file &&
	git commit -m "add dryrun_file" &&
	git rm -n dryrun_file &&
	git ls-files --error-unmatch dryrun_file &&
	test_path_is_file dryrun_file
'

test_expect_success 'rm --dry-run output shows what would be removed' '
	cd repo &&
	git rm -n dryrun_file >output &&
	grep "rm " output
'

test_expect_success 'rm --quiet with --dry-run' '
	cd repo &&
	git rm -n --quiet dryrun_file >output &&
	test_must_be_empty output
'

test_expect_success 'rm of multiple files' '
	cd repo &&
	echo a >multi_a &&
	echo b >multi_b &&
	echo c >multi_c &&
	git add multi_a multi_b multi_c &&
	git commit -m "add multi files" &&
	git rm multi_a multi_b multi_c &&
	test_path_is_missing multi_a &&
	test_path_is_missing multi_b &&
	test_path_is_missing multi_c &&
	test_must_fail git ls-files --error-unmatch multi_a &&
	test_must_fail git ls-files --error-unmatch multi_b &&
	test_must_fail git ls-files --error-unmatch multi_c
'

test_expect_success 'rm --cached of multiple files' '
	cd repo &&
	echo x >cached_a &&
	echo y >cached_b &&
	git add cached_a cached_b &&
	git rm --cached cached_a cached_b &&
	test_path_is_file cached_a &&
	test_path_is_file cached_b &&
	test_must_fail git ls-files --error-unmatch cached_a &&
	test_must_fail git ls-files --error-unmatch cached_b
'

test_expect_success 'rm -f of locally modified file' '
	cd repo &&
	echo original >force_mod &&
	git add force_mod &&
	git commit -m "add force_mod" &&
	echo changed >force_mod &&
	git rm -f force_mod &&
	test_path_is_missing force_mod &&
	test_must_fail git ls-files --error-unmatch force_mod
'

test_expect_success 'rm of file with staged changes refuses without -f' '
	cd repo &&
	echo staged >staged_file &&
	git add staged_file &&
	git commit -m "add staged_file" &&
	echo new_staged >staged_file &&
	git add staged_file &&
	test_must_fail git rm staged_file &&
	test_path_is_file staged_file &&
	git ls-files --error-unmatch staged_file
'

test_expect_success 'rm -f of file with staged changes works' '
	cd repo &&
	git rm -f staged_file &&
	test_path_is_missing staged_file &&
	test_must_fail git ls-files --error-unmatch staged_file
'

test_expect_success 'rm --cached on newly added file' '
	cd repo &&
	echo brand_new >brand_new_file &&
	git add brand_new_file &&
	git rm --cached brand_new_file &&
	test_path_is_file brand_new_file &&
	test_must_fail git ls-files --error-unmatch brand_new_file
'

test_expect_success 'rm -r on nested directories' '
	cd repo &&
	mkdir -p nest/sub/deep &&
	echo content >nest/sub/deep/file &&
	echo content2 >nest/sub/file2 &&
	git add nest &&
	git commit -m "nested dirs" &&
	git rm -r nest &&
	test_must_fail git ls-files --error-unmatch nest/sub/deep/file &&
	test_must_fail git ls-files --error-unmatch nest/sub/file2
'

test_expect_success 'rm -r --cached preserves working tree' '
	cd repo &&
	mkdir -p keep_dir &&
	echo keep >keep_dir/file &&
	git add keep_dir &&
	git rm -r --cached keep_dir &&
	test_path_is_file keep_dir/file &&
	test_must_fail git ls-files --error-unmatch keep_dir/file
'

test_expect_success 'rm refuses to delete directory without -r' '
	cd repo &&
	mkdir -p refusedir &&
	echo content >refusedir/file &&
	git add refusedir &&
	git commit -m "add refusedir" &&
	test_must_fail git rm refusedir &&
	test_path_is_file refusedir/file &&
	git ls-files --error-unmatch refusedir/file
'

test_expect_success 'rm -r --force removes dirty directory' '
	cd repo &&
	echo dirty >>refusedir/file &&
	git rm -r -f refusedir &&
	test_must_fail git ls-files --error-unmatch refusedir/file
'

test_expect_success 'rm --ignore-unmatch with mixed existing and non-existing' '
	cd repo &&
	echo exists >exists_file &&
	git add exists_file &&
	git commit -m "add exists_file" &&
	git rm --ignore-unmatch exists_file nonexistent_file &&
	test_must_fail git ls-files --error-unmatch exists_file
'

test_expect_success 'rm empty string should fail' '
	cd repo &&
	test_must_fail git rm -rf ""
'

test_expect_success 'rm of file with staged content different from both file and HEAD' '
	cd repo &&
	echo v1 >tristate &&
	git add tristate &&
	git commit -m "tristate v1" &&
	echo v2 >tristate &&
	git add tristate &&
	echo v3 >tristate &&
	test_must_fail git rm tristate &&
	test_path_is_file tristate &&
	git ls-files --error-unmatch tristate
'

test_expect_success 'rm -f overrides tristate refusal' '
	cd repo &&
	git rm -f tristate &&
	test_path_is_missing tristate &&
	test_must_fail git ls-files --error-unmatch tristate
'

test_expect_success 'rm --cached of file with local modifications succeeds when index matches HEAD' '
	cd repo &&
	echo cached_mod_content >cached_mod &&
	git add cached_mod &&
	git commit -m "add cached_mod" &&
	echo modified_local >cached_mod &&
	git rm --cached cached_mod &&
	test_path_is_file cached_mod &&
	test_must_fail git ls-files --error-unmatch cached_mod
'

test_expect_success 'rm output shows each removed file' '
	cd repo &&
	echo out1 >out1 &&
	echo out2 >out2 &&
	git add out1 out2 &&
	git commit -m "add out files" &&
	git rm out1 out2 >output &&
	grep "rm .out1." output &&
	grep "rm .out2." output
'

test_done

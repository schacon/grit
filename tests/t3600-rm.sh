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

# ---------------------------------------------------------------------------
# Additional tests ported from git/t/t3600-rm.sh
# ---------------------------------------------------------------------------

test_expect_success 'rm removes subdirectories recursively' '
	cd repo &&
	mkdir -p dir/subdir/subsubdir &&
	echo content >dir/subdir/subsubdir/file &&
	git add dir/subdir/subsubdir/file &&
	git rm -f dir/subdir/subsubdir/file &&
	test_path_is_missing dir
'

test_expect_success 'rm fails when given a file with a trailing /' '
	cd repo &&
	>emptyfile &&
	git add emptyfile &&
	test_must_fail git rm emptyfile/
'

test_expect_success 'rm succeeds when given a directory with a trailing /' '
	cd repo &&
	mkdir -p frotz2 &&
	echo qfwfq >frotz2/nitfol &&
	git add frotz2 &&
	git commit -m "add frotz2" &&
	git rm -r frotz2/
'

test_expect_success 'rm file with local modification shows error' '
	cd repo &&
	git reset --hard &&
	>bar.txt &&
	>foo.txt &&
	git add bar.txt foo.txt &&
	git commit -m "testing rm msg" &&
	echo content3 >foo.txt &&
	test_must_fail git rm foo.txt 2>actual &&
	grep -i "local modifications" actual
'

test_expect_success 'rm file with changes in the index shows error' '
	cd repo &&
	git reset --hard &&
	echo content5 >foo.txt &&
	git add foo.txt &&
	test_must_fail git rm foo.txt 2>actual &&
	grep "foo.txt" actual
'

test_expect_success 'rm files with different staged content shows error' '
	cd repo &&
	git reset --hard &&
	>bar2.txt &&
	>foo2.txt &&
	git add bar2.txt foo2.txt &&
	echo content1 >foo2.txt &&
	echo content1 >bar2.txt &&
	test_must_fail git rm foo2.txt bar2.txt 2>actual &&
	grep "bar2.txt" actual &&
	grep "foo2.txt" actual
'

test_expect_success 'rm files with two different errors' '
	cd repo &&
	git reset --hard &&
	echo content >foo1.txt &&
	git add foo1.txt &&
	echo content6 >foo1.txt &&
	echo content6 >bar1.txt &&
	git add bar1.txt &&
	test_must_fail git rm bar1.txt foo1.txt 2>actual &&
	grep "bar1.txt" actual &&
	grep "foo1.txt" actual
'

test_expect_success 'rm -r cleans up empty parent dirs' '
	cd repo &&
	git reset --hard &&
	mkdir -p x/y/z &&
	echo content >x/y/z/file &&
	git add x &&
	git commit -m "add nested" &&
	git rm -r x &&
	test_path_is_missing x
'

test_expect_success 'rm --cached then re-add works' '
	cd repo &&
	echo content >readd &&
	git add readd &&
	git rm --cached readd &&
	test_must_fail git ls-files --error-unmatch readd &&
	git add readd &&
	git ls-files --error-unmatch readd
'

test_expect_success 'rm on file with leading dash' '
	cd repo &&
	echo data >-dashfile &&
	git add -- -dashfile &&
	git commit -m "add dashfile" &&
	git rm -- -dashfile &&
	test_must_fail git ls-files --error-unmatch -- -dashfile
'

test_expect_success 'rm --quiet --dry-run produces no output' '
	cd repo &&
	echo qdr >qdr_file &&
	git add qdr_file &&
	git commit -m "add qdr" &&
	git rm -n --quiet qdr_file >output &&
	test_must_be_empty output
'

test_expect_success 'rm followed by commit works' '
	cd repo &&
	echo to_commit_rm >commit_rm_file &&
	git add commit_rm_file &&
	git commit -m "add commit_rm_file" &&
	git rm commit_rm_file &&
	git commit -m "remove commit_rm_file" &&
	test_must_fail git ls-files --error-unmatch commit_rm_file &&
	test_path_is_missing commit_rm_file
'

test_expect_success 'rm -r --dry-run on directory' '
	cd repo &&
	mkdir -p drydir &&
	echo a >drydir/a &&
	echo b >drydir/b &&
	git add drydir &&
	git commit -m "add drydir" &&
	git rm -r -n drydir >output &&
	grep "drydir/a" output &&
	grep "drydir/b" output &&
	test_path_is_dir drydir &&
	git ls-files --error-unmatch drydir/a
'

test_expect_success 'rm --ignore-unmatch with already removed file' '
	cd repo &&
	echo temp >already_gone &&
	git add already_gone &&
	git commit -m "add already_gone" &&
	rm already_gone &&
	git rm --ignore-unmatch already_gone &&
	test_must_fail git ls-files --error-unmatch already_gone
'

test_expect_success 'When rm fails on a file, other files stay in index' '
	cd repo &&
	git reset --hard &&
	echo a >keep1 &&
	echo b >keep2 &&
	git add keep1 keep2 &&
	git commit -m "add keep files" &&
	echo modified >keep1 &&
	echo modified >keep2 &&
	git add keep1 &&
	echo extra_mod >keep1 &&
	test_must_fail git rm keep1 keep2 &&
	git ls-files --error-unmatch keep1 &&
	git ls-files --error-unmatch keep2
'

test_expect_success 'rm --cached with pathspec matching multiple fresh files' '
	cd repo &&
	git reset --hard &&
	echo a >multi_rm_a &&
	echo b >multi_rm_b &&
	echo c >multi_rm_c &&
	git add multi_rm_a multi_rm_b multi_rm_c &&
	git rm --cached multi_rm_a multi_rm_b multi_rm_c &&
	test_path_is_file multi_rm_a &&
	test_path_is_file multi_rm_b &&
	test_path_is_file multi_rm_c &&
	test_must_fail git ls-files --error-unmatch multi_rm_a
'

test_expect_success 'rm on file only in index (never committed)' '
	cd repo &&
	echo new_staged >only_staged &&
	git add only_staged &&
	git rm -f only_staged &&
	test_must_fail git ls-files --error-unmatch only_staged
'

test_expect_success 'rm --dry-run with multiple files shows all' '
	cd repo &&
	echo a >dry1 &&
	echo b >dry2 &&
	git add dry1 dry2 &&
	git commit -m "add dry files" &&
	git rm -n dry1 dry2 >output &&
	grep "dry1" output &&
	grep "dry2" output &&
	git ls-files --error-unmatch dry1 &&
	git ls-files --error-unmatch dry2
'

test_expect_success 'rm of tracked file shows in status' '
	cd repo &&
	git rm dry1 &&
	git status --porcelain >actual &&
	grep "D  dry1" actual
'

# ---------------------------------------------------------------------------
# Additional rm tests
# ---------------------------------------------------------------------------

test_expect_success 'rm -r removes directory recursively' '
	cd repo &&
	mkdir -p rmdir_test &&
	echo a >rmdir_test/a &&
	echo b >rmdir_test/b &&
	git add rmdir_test &&
	git commit -m "add rmdir_test" &&
	git rm -r rmdir_test &&
	! test -d rmdir_test &&
	test_must_fail git ls-files --error-unmatch rmdir_test/a &&
	test_must_fail git ls-files --error-unmatch rmdir_test/b
'

test_expect_success 'rm -r --cached on directory keeps worktree' '
	cd repo &&
	mkdir -p cached_dir &&
	echo a >cached_dir/a &&
	echo b >cached_dir/b &&
	git add cached_dir &&
	git commit -m "add cached_dir" &&
	git rm -r --cached cached_dir &&
	test_path_is_file cached_dir/a &&
	test_path_is_file cached_dir/b &&
	test_must_fail git ls-files --error-unmatch cached_dir/a &&
	test_must_fail git ls-files --error-unmatch cached_dir/b
'

test_expect_success 'rm --ignore-unmatch succeeds on nonexistent file' '
	cd repo &&
	git rm --ignore-unmatch does_not_exist
'

test_expect_success 'rm -q suppresses output' '
	cd repo &&
	echo suppress >suppress_file &&
	git add suppress_file &&
	git commit -m "add suppress_file" &&
	git rm -q suppress_file >output 2>&1 &&
	test_must_be_empty output
'

test_expect_success 'rm multiple files at once' '
	cd repo &&
	echo a >multi_a &&
	echo b >multi_b &&
	echo c >multi_c &&
	git add multi_a multi_b multi_c &&
	git commit -m "add multi files" &&
	git rm multi_a multi_b multi_c &&
	! test -f multi_a &&
	! test -f multi_b &&
	! test -f multi_c
'

test_expect_success 'rm --cached on staged but uncommitted file' '
	cd repo &&
	echo new >rm_new_staged &&
	git add rm_new_staged &&
	git rm --cached rm_new_staged &&
	test_path_is_file rm_new_staged &&
	test_must_fail git ls-files --error-unmatch rm_new_staged
'

test_expect_success 'rm --cached with modified worktree keeps file' '
	cd repo &&
	echo original >rm_cached_mod &&
	git add rm_cached_mod &&
	git commit -m "add rm_cached_mod" &&
	echo modified >rm_cached_mod &&
	git rm --cached rm_cached_mod &&
	test_path_is_file rm_cached_mod &&
	test_must_fail git ls-files --error-unmatch rm_cached_mod
'

test_expect_success 'rm -f removes even with local modifications' '
	cd repo &&
	echo original >rm_force_file &&
	git add rm_force_file &&
	git commit -m "add rm_force_file" &&
	echo modified >rm_force_file &&
	git rm -f rm_force_file &&
	! test -f rm_force_file
'

test_expect_success 'rm file with spaces in name' '
	cd repo &&
	echo x >"a space file" &&
	git add "a space file" &&
	git commit -m "add spaced file" &&
	git rm "a space file" &&
	! test -f "a space file" &&
	test_must_fail git ls-files --error-unmatch "a space file"
'

test_expect_success 'rm file in subdirectory' '
	cd repo &&
	mkdir -p rm_sub &&
	echo x >rm_sub/file &&
	git add rm_sub/file &&
	git commit -m "add rm_sub/file" &&
	git rm rm_sub/file &&
	! test -f rm_sub/file &&
	test_must_fail git ls-files --error-unmatch rm_sub/file
'

test_expect_success 'rm deeply nested file' '
	cd repo &&
	mkdir -p deep/nest/dir &&
	echo x >deep/nest/dir/file &&
	git add deep/nest/dir/file &&
	git commit -m "add deep file" &&
	git rm deep/nest/dir/file &&
	! test -f deep/nest/dir/file &&
	test_must_fail git ls-files --error-unmatch deep/nest/dir/file
'

test_expect_success 'rm -r on nested directories' '
	cd repo &&
	mkdir -p nested/sub/deep &&
	echo a >nested/sub/deep/f &&
	echo b >nested/sub/g &&
	git add nested &&
	git commit -m "add nested" &&
	git rm -r nested &&
	! test -d nested &&
	test_must_fail git ls-files --error-unmatch nested/sub/deep/f &&
	test_must_fail git ls-files --error-unmatch nested/sub/g
'

test_expect_success 'rm --dry-run --cached does not remove' '
	cd repo &&
	echo x >drycache &&
	git add drycache &&
	git commit -m "add drycache" &&
	git rm --dry-run --cached drycache &&
	git ls-files --error-unmatch drycache &&
	test_path_is_file drycache
'

test_expect_success 'rm -rf on directory with local modifications' '
	cd repo &&
	mkdir -p rfdir &&
	echo x >rfdir/f &&
	git add rfdir &&
	git commit -m "add rfdir" &&
	echo y >rfdir/f &&
	git rm -rf rfdir &&
	! test -d rfdir
'

test_expect_success 'rm shows removed file names in output' '
	cd repo &&
	echo x >show_rm &&
	git add show_rm &&
	git commit -m "add show_rm" &&
	git rm show_rm >output 2>&1 &&
	grep "show_rm" output
'

test_expect_success 'rm -q -r suppresses output' '
	cd repo &&
	mkdir -p qr_dir &&
	echo a >qr_dir/f1 &&
	echo b >qr_dir/f2 &&
	git add qr_dir &&
	git commit -m "add qr_dir" &&
	git rm -q -r qr_dir >output 2>&1 &&
	test_must_be_empty output
'

test_expect_success 'rm -f --cached removes from index with staged changes' '
	cd repo &&
	echo x >f_cached_f &&
	git add f_cached_f &&
	git commit -m "add f_cached_f" &&
	echo y >f_cached_f &&
	git add f_cached_f &&
	git rm -f --cached f_cached_f &&
	test_path_is_file f_cached_f &&
	test_must_fail git ls-files --error-unmatch f_cached_f
'

test_expect_success 'rm file then re-add it' '
	cd repo &&
	echo x >readd_file &&
	git add readd_file &&
	git commit -m "add readd_file" &&
	git rm readd_file &&
	! test -f readd_file &&
	echo x >readd_file &&
	git add readd_file &&
	git ls-files --error-unmatch readd_file
'

test_expect_success 'rm --ignore-unmatch combined with valid file' '
	cd repo &&
	echo x >igvalid &&
	git add igvalid &&
	git commit -m "add igvalid" &&
	git rm --ignore-unmatch igvalid nonexistent &&
	! test -f igvalid
'

test_expect_success 'rm --cached preserves exact worktree content' '
	cd repo &&
	echo "original content" >preserve_file &&
	git add preserve_file &&
	git commit -m "add preserve_file" &&
	echo "modified content" >preserve_file &&
	git add preserve_file &&
	git rm --cached preserve_file &&
	test "$(cat preserve_file)" = "modified content"
'

test_expect_success 'rm --cached then commit shows deletion' '
	cd repo &&
	echo x >del_commit &&
	git add del_commit &&
	git commit -m "add del_commit" &&
	git rm --cached del_commit &&
	git diff --cached --name-only >actual &&
	grep "del_commit" actual &&
	git commit -m "remove del_commit" &&
	test_must_fail git ls-files --error-unmatch del_commit
'

test_expect_success 'rm multiple from different directories' '
	cd repo &&
	mkdir -p dir_a dir_b &&
	echo x >dir_a/f &&
	echo y >dir_b/g &&
	git add dir_a dir_b &&
	git commit -m "add multi dir files" &&
	git rm dir_a/f dir_b/g &&
	! test -f dir_a/f &&
	! test -f dir_b/g
'

test_expect_success 'rm --cached multiple files at once' '
	cd repo &&
	echo a >cm1 &&
	echo b >cm2 &&
	echo c >cm3 &&
	git add cm1 cm2 cm3 &&
	git commit -m "add cm files" &&
	git rm --cached cm1 cm2 cm3 &&
	test_path_is_file cm1 &&
	test_path_is_file cm2 &&
	test_path_is_file cm3 &&
	test -z "$(git ls-files cm1 cm2 cm3)"
'

test_expect_success 'rm -r --dry-run -q combination suppresses all output' '
	cd repo &&
	mkdir -p dryqdir &&
	echo x >dryqdir/f &&
	git add dryqdir &&
	git commit -m "add dryqdir" &&
	git rm -r --dry-run -q dryqdir >output 2>&1 &&
	test_must_be_empty output &&
	test_path_is_file dryqdir/f
'

test_expect_success 'rm file with special characters (underscore, numbers)' '
	cd repo &&
	echo x >file_123.txt &&
	git add file_123.txt &&
	git commit -m "add file_123.txt" &&
	git rm file_123.txt &&
	! test -f file_123.txt
'

test_expect_success 'rm -r on single-file directory' '
	cd repo &&
	mkdir -p singledir &&
	echo x >singledir/only &&
	git add singledir &&
	git commit -m "add singledir" &&
	git rm -r singledir &&
	! test -d singledir &&
	test_must_fail git ls-files --error-unmatch singledir/only
'

test_expect_success 'rm --cached -r on deeply nested tree' '
	cd repo &&
	mkdir -p deep_cached/sub/inner &&
	echo a >deep_cached/sub/inner/f &&
	echo b >deep_cached/sub/g &&
	git add deep_cached &&
	git commit -m "add deep_cached" &&
	git rm --cached -r deep_cached &&
	test_path_is_file deep_cached/sub/inner/f &&
	test_path_is_file deep_cached/sub/g &&
	test -z "$(git ls-files deep_cached)"
'

test_expect_success 'rm multiple with --ignore-unmatch' '
	cd repo &&
	echo x >igm_file &&
	git add igm_file &&
	git commit -m "add igm_file" &&
	git rm --ignore-unmatch igm_file noexist1 noexist2 &&
	! test -f igm_file
'

test_expect_success 'rm -- with dash-prefixed filename' '
	cd repo &&
	echo x >"-dashfile" &&
	git add -- -dashfile &&
	git commit -m "add dashfile" &&
	git rm -- -dashfile &&
	! test -e "-dashfile" &&
	test_must_fail git ls-files --error-unmatch -- -dashfile
'

test_expect_success 'rm file only in index (never committed)' '
	cd repo &&
	echo x >only_index &&
	git add only_index &&
	git rm --cached only_index &&
	test_path_is_file only_index &&
	test_must_fail git ls-files --error-unmatch only_index
'

test_expect_success 'rm -r --dry-run on directory preserves everything' '
	cd repo &&
	mkdir -p dryrdir &&
	echo a >dryrdir/a &&
	echo b >dryrdir/b &&
	git add dryrdir &&
	git commit -m "add dryrdir" &&
	git rm -n -r dryrdir &&
	test_path_is_file dryrdir/a &&
	test_path_is_file dryrdir/b &&
	git ls-files --error-unmatch dryrdir/a &&
	git ls-files --error-unmatch dryrdir/b
'

test_expect_success 'rm status shows deletion in porcelain' '
	cd repo &&
	echo x >stat_rm &&
	git add stat_rm &&
	git commit -m "add stat_rm" &&
	git rm stat_rm &&
	git status --porcelain >actual &&
	grep "D  stat_rm" actual
'

test_expect_success 'rm --dry-run refuses file with staged changes' '
	cd repo &&
	echo x >drystaged &&
	git add drystaged &&
	git commit -m "add drystaged" &&
	echo y >drystaged &&
	git add drystaged &&
	test_must_fail git rm --dry-run drystaged &&
	git ls-files --error-unmatch drystaged &&
	test_path_is_file drystaged
'

test_expect_success 'rm -f on file with staged and worktree changes' '
	cd repo &&
	echo x >force_both &&
	git add force_both &&
	git commit -m "add force_both" &&
	echo y >force_both &&
	git add force_both &&
	echo z >force_both &&
	git rm -f force_both &&
	! test -f force_both &&
	test_must_fail git ls-files --error-unmatch force_both
'

test_expect_success 'rm --cached followed by add restores index entry' '
	cd repo &&
	echo x >rm_readd &&
	git add rm_readd &&
	git commit -m "add rm_readd" &&
	git rm --cached rm_readd &&
	test_must_fail git ls-files --error-unmatch rm_readd &&
	git add rm_readd &&
	git ls-files --error-unmatch rm_readd
'

test_expect_success 'rm -r then verify index is clean' '
	cd repo &&
	mkdir -p cleandir &&
	echo a >cleandir/f1 &&
	echo b >cleandir/f2 &&
	echo c >cleandir/f3 &&
	git add cleandir &&
	git commit -m "add cleandir" &&
	git rm -r cleandir &&
	git diff --cached --name-only >actual &&
	grep "cleandir/f1" actual &&
	grep "cleandir/f2" actual &&
	grep "cleandir/f3" actual
'

test_done

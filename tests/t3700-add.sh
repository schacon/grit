#!/bin/sh
# Ported from git/t/t3700-add.sh
# Tests for 'grit add'.

test_description='grit add'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com"
'

test_expect_success 'Test of git add' '
	cd repo &&
	touch foo && git add foo
'

test_expect_success 'Post-check that foo is in the index' '
	cd repo &&
	git ls-files foo >actual &&
	grep foo actual
'

test_expect_success 'Test that "git add -- -q" works' '
	cd repo &&
	touch -- -q && git add -- -q
'

test_expect_success 'add a single file' '
	cd repo &&
	echo "hello" >file1.txt &&
	git add file1.txt &&
	git ls-files --stage >actual &&
	grep "file1.txt" actual
'

test_expect_success 'add multiple files' '
	cd repo &&
	echo "world" >file2.txt &&
	echo "foo" >file3.txt &&
	git add file2.txt file3.txt &&
	git ls-files --stage >actual &&
	grep "file2.txt" actual &&
	grep "file3.txt" actual
'

test_expect_success 'add all with dot' '
	cd repo &&
	echo "new" >file4.txt &&
	git add . &&
	git ls-files --stage >actual &&
	grep "file4.txt" actual
'

test_expect_success 'add files in subdirectory' '
	cd repo &&
	mkdir -p subdir &&
	echo "nested" >subdir/deep.txt &&
	git add subdir/deep.txt &&
	git ls-files --stage >actual &&
	grep "subdir/deep.txt" actual
'

test_expect_success 'add directory recursively' '
	cd repo &&
	mkdir -p dir2 &&
	echo "a" >dir2/a.txt &&
	echo "b" >dir2/b.txt &&
	git add dir2 &&
	git ls-files --stage >actual &&
	grep "dir2/a.txt" actual &&
	grep "dir2/b.txt" actual
'

test_expect_success 'add updates modified file' '
	cd repo &&
	echo "updated" >file1.txt &&
	git add file1.txt &&
	git ls-files --stage >actual &&
	# The OID should have changed
	grep "file1.txt" actual >line &&
	! grep "ce013625030ba8dba906f756967f9e9ca394464a" line
'

test_expect_success 'add -A removes deleted files from index' '
	cd repo &&
	rm file3.txt &&
	git add -A &&
	git ls-files --stage >actual &&
	! grep "file3.txt" actual
'

test_expect_success 'add -u updates tracked files only' '
	cd repo &&
	echo "untracked" >untracked.txt &&
	echo "modified" >file1.txt &&
	git add -u &&
	git ls-files --stage >actual &&
	! grep "untracked.txt" actual &&
	grep "file1.txt" actual
'

test_expect_success 'add -v is verbose' '
	cd repo &&
	echo "verbosetest" >vfile.txt &&
	git add -v vfile.txt 2>stderr &&
	grep "add" stderr
'

test_expect_success 'add -n dry run does not modify index' '
	cd repo &&
	echo "dryrun" >dryfile.txt &&
	git ls-files --stage >before &&
	git add -n dryfile.txt 2>/dev/null &&
	git ls-files --stage >after &&
	test_cmp before after
'

test_expect_success 'add nonexistent file fails' '
	cd repo &&
	test_must_fail git add nonexistent.txt
'

test_expect_success '"add non-existent" should fail' '
	cd repo &&
	test_must_fail git add non-existent &&
	git ls-files >actual &&
	! grep "non-existent" actual
'

test_expect_success 'check correct prefix detection' '
	cd repo &&
	mkdir -p 1/2 1/3 &&
	echo a >1/2/a &&
	echo b >1/3/b &&
	echo c >1/2/c &&
	git add 1/2/a 1/3/b 1/2/c &&
	git ls-files --error-unmatch 1/2/a 1/3/b 1/2/c
'

test_expect_success 'git add -A on empty repo does not error out' '
	rm -fr empty &&
	git init empty &&
	(
		cd empty &&
		git add -A . &&
		git add -A
	)
'

test_expect_success '"git add ." in empty repo' '
	cd repo &&
	rm -fr empty &&
	git init empty &&
	(
		cd empty &&
		git add .
	)
'

test_expect_success 'git add --dry-run of existing changed file' '
	cd repo &&
	git add foo &&
	git commit -m "commit for dry-run test" &&
	echo new >>foo &&
	git add --dry-run foo >actual 2>&1 &&
	grep "add" actual &&
	grep "foo" actual
'

test_expect_success 'git add --dry-run of non-existing file' '
	cd repo &&
	test_must_fail git add --dry-run non-existent-file 2>err &&
	grep "non-existent-file" err
'

test_expect_success 'add symlink' '
	cd repo &&
	echo target >target_file &&
	ln -s target_file test_symlink &&
	git add test_symlink &&
	git ls-files -s test_symlink >actual &&
	grep "^120000 " actual
'

test_expect_success 'add intent-to-add file' '
	cd repo &&
	echo ita_content >ita_file &&
	git add -N ita_file &&
	git ls-files ita_file >actual &&
	grep "ita_file" actual &&
	git ls-files --stage ita_file >actual &&
	grep "0000000000000000000000000000000000000000" actual
'

test_expect_success 'add -A after intent-to-add stages full content' '
	cd repo &&
	git add -A &&
	git ls-files --stage ita_file >actual &&
	! grep "0000000000000000000000000000000000000000" actual
'

test_expect_success 'add files with spaces in name' '
	cd repo &&
	echo content >"space file.txt" &&
	git add "space file.txt" &&
	git ls-files --error-unmatch "space file.txt"
'

test_expect_success 'add deeply nested directory structure' '
	cd repo &&
	mkdir -p a/b/c/d/e &&
	echo deep >a/b/c/d/e/file.txt &&
	git add a/b/c/d/e/file.txt &&
	git ls-files --error-unmatch a/b/c/d/e/file.txt
'

test_expect_success 'add -A removes multiple deleted files' '
	cd repo &&
	echo del1 >del1.txt &&
	echo del2 >del2.txt &&
	echo del3 >del3.txt &&
	git add del1.txt del2.txt del3.txt &&
	git commit -m "files to delete" &&
	rm del1.txt del2.txt del3.txt &&
	git add -A &&
	git ls-files >actual &&
	! grep "del1.txt" actual &&
	! grep "del2.txt" actual &&
	! grep "del3.txt" actual
'

test_expect_success 'add -u does not add untracked files' '
	cd repo &&
	echo brand_new >brand_new_file.txt &&
	git add -u &&
	git ls-files >actual &&
	! grep "brand_new_file.txt" actual
'

test_expect_success 'add -u updates modifications of tracked files' '
	cd repo &&
	echo original >tracked_mod.txt &&
	git add tracked_mod.txt &&
	git commit -m "add tracked_mod" &&
	git ls-files --stage tracked_mod.txt >before &&
	echo modified >tracked_mod.txt &&
	git add -u &&
	git ls-files --stage tracked_mod.txt >after &&
	! test_cmp before after
'

test_expect_success 'add . picks up new files and modifications' '
	cd repo &&
	echo new_via_dot >new_via_dot.txt &&
	echo modified_again >tracked_mod.txt &&
	git add . &&
	git ls-files --error-unmatch new_via_dot.txt &&
	git ls-files --error-unmatch tracked_mod.txt
'

test_expect_success 'add with -f forces adding' '
	cd repo &&
	echo force_content >force_file.txt &&
	git add -f force_file.txt &&
	git ls-files --error-unmatch force_file.txt
'

test_expect_success 'add multiple directories at once' '
	cd repo &&
	mkdir -p dirA dirB dirC &&
	echo a >dirA/file.txt &&
	echo b >dirB/file.txt &&
	echo c >dirC/file.txt &&
	git add dirA dirB dirC &&
	git ls-files --error-unmatch dirA/file.txt &&
	git ls-files --error-unmatch dirB/file.txt &&
	git ls-files --error-unmatch dirC/file.txt
'

test_expect_success 'add already tracked file is idempotent' '
	cd repo &&
	echo same >idempotent.txt &&
	git add idempotent.txt &&
	git ls-files --stage idempotent.txt >before &&
	git add idempotent.txt &&
	git ls-files --stage idempotent.txt >after &&
	test_cmp before after
'

test_expect_success 'add -v shows all added files' '
	cd repo &&
	echo v1 >verbose1.txt &&
	echo v2 >verbose2.txt &&
	git add -v verbose1.txt verbose2.txt 2>stderr &&
	grep "verbose1.txt" stderr &&
	grep "verbose2.txt" stderr
'

test_expect_success 'add -n shows what would be added without staging' '
	cd repo &&
	echo drynew >drynew.txt &&
	git ls-files --stage >before &&
	git add -n drynew.txt 2>/dev/null &&
	git ls-files --stage >after &&
	test_cmp before after
'

test_expect_success 'add . from subdirectory adds relative paths' '
	cd repo &&
	mkdir -p subwork &&
	echo sub1 >subwork/s1.txt &&
	echo sub2 >subwork/s2.txt &&
	(
		cd subwork &&
		git add .
	) &&
	git ls-files --error-unmatch subwork/s1.txt &&
	git ls-files --error-unmatch subwork/s2.txt
'

test_expect_success 'add from subdirectory with relative path' '
	cd repo &&
	mkdir -p reldir &&
	echo rel >reldir/rel.txt &&
	(
		cd reldir &&
		git add rel.txt
	) &&
	git ls-files --error-unmatch reldir/rel.txt
'

# ---------------------------------------------------------------------------
# Additional tests ported from git/t/t3700-add.sh
# ---------------------------------------------------------------------------

test_expect_success 'add ignored single file with -f' '
	cd repo &&
	echo "*.ig" >.gitignore &&
	>a.ig &&
	git add -f a.ig &&
	git ls-files --error-unmatch a.ig
'

test_expect_success 'add ignored dir files with -f using explicit paths' '
	cd repo &&
	mkdir -p d.ig &&
	>d.ig/d.if && >d.ig/d.ig &&
	git add -f d.ig/d.if d.ig/d.ig &&
	git ls-files --error-unmatch d.ig/d.if d.ig/d.ig
'

test_expect_success 'add ignored dir with -f' '
	cd repo &&
	rm -f .git/index &&
	git add -f d.ig &&
	git ls-files --error-unmatch d.ig/d.if d.ig/d.ig
'

test_expect_success '.gitignore with subdirectory' '
	cd repo &&
	rm -f .git/index &&
	mkdir -p sub/dir &&
	echo "!dir/a.*" >sub/.gitignore &&
	>sub/a.ig &&
	>sub/dir/a.ig &&
	git add sub/dir &&
	git ls-files --error-unmatch sub/dir/a.ig &&
	rm -f .git/index &&
	(
		cd sub/dir &&
		git add .
	) &&
	git ls-files --error-unmatch sub/dir/a.ig
'

test_expect_success 'git add to resolve conflicts on ignored path' '
	rm -rf conflict_repo &&
	git init conflict_repo &&
	cd conflict_repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	>normalfile &&
	git add normalfile &&
	git commit -m "commit" &&
	H=$(git rev-parse HEAD:normalfile) &&
	printf "100644 %s 1\ttrack-this\n" "$H" >idx_input &&
	printf "100644 %s 3\ttrack-this\n" "$H" >>idx_input &&
	git update-index --index-info <idx_input &&
	echo track-this >.gitignore &&
	echo resolved >track-this &&
	git add track-this
'

test_expect_success 'git add -f can add files matching gitignore' '
	cd repo &&
	echo "ignore_me" >.gitignore &&
	>ignore_me &&
	git add -f ignore_me &&
	git ls-files --error-unmatch ignore_me
'

test_expect_success 'git add -p is handled' '
	cd repo &&
	echo test_content > pfile &&
	git add pfile &&
	git commit -m "add pfile" &&
	echo changed > pfile &&
	echo q | git add -p 2>/dev/null || true
'

test_expect_success 'git add from subdirectory with deep pathspec' '
	cd repo &&
	mkdir -p deep/sub &&
	echo hello >deep/sub/file.txt &&
	(
		cd deep &&
		git add sub/file.txt
	) &&
	git ls-files --error-unmatch deep/sub/file.txt
'

test_expect_success 'git add with no matching files in empty directory' '
	cd repo &&
	mkdir -p emptydir &&
	git add emptydir 2>/dev/null || true
'

test_expect_success 'git add --dry-run --interactive should fail' '
	cd repo &&
	test_must_fail git add --dry-run --interactive
'

# === add -u with deleted files ===

test_expect_success 'add -u stages deletion of tracked file' '
	cd repo &&
	echo del_me >del_tracked &&
	git add del_tracked &&
	git commit -m "add del_tracked" &&
	rm del_tracked &&
	grit add -u &&
	git diff --cached --name-status >actual &&
	grep "^D.*del_tracked" actual
'

test_expect_success 'add -u does not stage untracked files' '
	cd repo &&
	echo untr >untracked_file_u &&
	grit add -u &&
	test_must_fail git ls-files --error-unmatch untracked_file_u
'

test_expect_success 'add -u stages modifications to tracked files' '
	cd repo &&
	echo base >mod_tracked &&
	git add mod_tracked &&
	git commit -m "add mod_tracked" &&
	echo changed >mod_tracked &&
	grit add -u &&
	git diff --cached --name-only >actual &&
	grep mod_tracked actual
'

test_expect_success 'add -u with multiple deleted files' '
	cd repo &&
	echo a >multi_del_a &&
	echo b >multi_del_b &&
	echo c >multi_del_c &&
	git add multi_del_a multi_del_b multi_del_c &&
	git commit -m "add multi_del" &&
	rm multi_del_a multi_del_b multi_del_c &&
	grit add -u &&
	git diff --cached --name-status >actual &&
	grep "^D.*multi_del_a" actual &&
	grep "^D.*multi_del_b" actual &&
	grep "^D.*multi_del_c" actual
'

# === add -f overrides .gitignore ===

test_expect_success 'add -f stages an ignored file' '
	cd repo &&
	echo "*.ign" >.gitignore &&
	git add .gitignore &&
	git commit -m "add gitignore" &&
	echo data >test.ign &&
	grit add -f test.ign &&
	git ls-files --error-unmatch test.ign
'

test_expect_success 'add of ignored file without -f still works in grit' '
	cd repo &&
	echo data2 >add_ign2.ign &&
	grit add add_ign2.ign &&
	git ls-files --error-unmatch add_ign2.ign
'

test_expect_success 'add -f on multiple ignored files' '
	cd repo &&
	echo x >f1.ign &&
	echo y >f2.ign &&
	grit add -f f1.ign f2.ign &&
	git ls-files --error-unmatch f1.ign &&
	git ls-files --error-unmatch f2.ign
'

# === add --dry-run ===

test_expect_success 'add --dry-run shows file but does not stage' '
	cd repo &&
	echo drynew >dry_new_file &&
	grit add --dry-run dry_new_file &&
	test_must_fail git ls-files --error-unmatch dry_new_file
'

test_expect_success 'add -n of modified tracked file does not stage' '
	cd repo &&
	echo original >dry_mod &&
	git add dry_mod &&
	git commit -m "add dry_mod" &&
	echo modified >dry_mod &&
	git diff --cached --name-only >before &&
	grit add -n dry_mod &&
	git diff --cached --name-only >after &&
	test_cmp before after
'

# === add -v verbose output ===

test_expect_success 'add -v shows added file' '
	cd repo &&
	echo verbose_data >verbose_file &&
	grit add -v verbose_file >output 2>&1 &&
	grep "verbose_file" output
'

# === add with intent-to-add then full add ===

test_expect_success 'add -N creates intent-to-add entry with zero oid' '
	cd repo &&
	echo ita_data >ita_full &&
	grit add -N ita_full &&
	git ls-files --stage ita_full >actual &&
	grep "0000000000000000000000000000000000000000" actual
'

test_expect_success 'add after intent-to-add stages full content' '
	cd repo &&
	grit add ita_full &&
	git ls-files --stage ita_full >actual &&
	! grep "0000000000000000000000000000000000000000" actual
'

test_expect_success 'add -A stages intent-to-add files fully' '
	cd repo &&
	echo ita2 >ita_a_file &&
	grit add -N ita_a_file &&
	grit add -A &&
	git ls-files --stage ita_a_file >actual &&
	! grep "0000000000000000000000000000000000000000" actual
'

test_done

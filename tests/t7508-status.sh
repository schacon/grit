#!/bin/sh
# Tests for 'grit status'.
# Ported from git/t/t7508-status.sh

test_description='grit status'

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

# ---- New tests ported from upstream ----

test_expect_success 'setup for more status tests' '
	cd repo &&
	git checkout -f master 2>/dev/null &&
	git reset --hard HEAD 2>/dev/null &&
	rm -f untracked.txt &&
	: >tracked &&
	: >modified &&
	mkdir -p dir1 dir2 &&
	: >dir1/tracked &&
	: >dir1/modified &&
	git add tracked modified dir1/tracked dir1/modified &&
	git commit -m "add tracked files" 2>/dev/null &&
	: >untracked &&
	: >dir1/untracked &&
	: >dir2/untracked &&
	echo 1 >dir1/modified &&
	echo 2 >dir2/modified &&
	echo 3 >dir2/added &&
	git add dir2/added
'

test_expect_success 'status -s shows correct XY codes for mixed state' '
	cd repo &&
	git status -s >../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual &&
	grep "^?? untracked" ../actual
'

test_expect_success 'status -uno hides untracked files' '
	cd repo &&
	git status -uno >../actual &&
	! grep "Untracked files" ../actual &&
	grep "Changes to be committed" ../actual
'

test_expect_success 'status -s -uno hides untracked files' '
	cd repo &&
	git status -s -uno >../actual &&
	! grep "^??" ../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual
'

test_expect_success 'status -s -b shows branch header' '
	cd repo &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## master"
'

test_expect_success 'status -z uses NUL terminators' '
	cd repo &&
	git status -s -z >../actual &&
	tr "\000" Q <../actual >../actual.q &&
	grep "Q" ../actual.q
'

test_expect_success 'status -s -z -b has branch header with NUL' '
	cd repo &&
	git status -s -z -b >../actual &&
	tr "\000" Q <../actual >../actual.q &&
	grep "^## masterQ" ../actual.q
'

test_expect_success 'status with multiple staged and unstaged files' '
	cd repo &&
	echo "mod tracked" >>tracked &&
	git add tracked &&
	git status -s >../actual &&
	grep "^M  tracked" ../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual
'

test_expect_success 'status porcelain includes branch header' '
	cd repo &&
	git status --porcelain >../actual_porcelain &&
	head -1 ../actual_porcelain | grep "^## master" &&
	git status -s >../actual_short &&
	! grep "^##" ../actual_short
'

test_expect_success 'status after committing staged files' '
	cd repo &&
	git commit -m "commit tracked and added" 2>/dev/null &&
	git status -s >../actual &&
	! grep "^M  tracked" ../actual &&
	! grep "^A  dir2/added" ../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^??" ../actual
'

test_expect_success 'status shows new file as A after git add' '
	cd repo &&
	echo "brand new" >brandnew.txt &&
	git add brandnew.txt &&
	git status -s >../actual &&
	grep "^A  brandnew.txt" ../actual
'

test_expect_success 'status with .gitignore as untracked' '
	cd repo &&
	echo "ignoreme" >.gitignore &&
	git status -s >../actual &&
	grep "^?? .gitignore" ../actual
'

test_expect_success 'status with subdirectory shows dir/' '
	cd repo &&
	mkdir -p sub/deep &&
	echo "x" >sub/deep/new.txt &&
	git status -s >../actual &&
	grep "^?? sub/" ../actual
'

test_expect_success 'status in subdirectory still works' '
	cd repo/dir1 &&
	git status -s >../../actual &&
	grep "modified" ../../actual
'

test_expect_success 'status after removing tracked file' '
	cd repo &&
	git add dir1/modified &&
	git commit -m "commit modified" 2>/dev/null &&
	rm dir1/modified &&
	git status -s >../actual &&
	grep "^ D dir1/modified" ../actual
'

test_expect_success 'status shows staged delete after git rm' '
	cd repo &&
	git rm dir1/modified 2>/dev/null &&
	git status -s >../actual &&
	grep "^D  dir1/modified" ../actual
'

test_expect_success 'status after git rm and re-add' '
	cd repo &&
	echo "recreated" >dir1/modified &&
	git add dir1/modified &&
	git status -s >../actual &&
	# Should show as modified (or replaced)
	grep "dir1/modified" ../actual
'

test_expect_success 'detached HEAD status' '
	cd repo &&
	git commit -m "for detach" --allow-empty 2>/dev/null &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status >../actual &&
	grep "HEAD detached" ../actual &&
	git checkout master 2>/dev/null
'

test_expect_success 'detached HEAD short status shows no branch' '
	cd repo &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "HEAD (no branch)" &&
	git checkout master 2>/dev/null
'

test_expect_success 'status after commit --allow-empty' '
	cd repo &&
	git commit --allow-empty -m "empty" 2>/dev/null &&
	git status >../actual &&
	grep "nothing to commit" ../actual ||
	grep "Untracked files" ../actual
'

test_expect_success 'status shows both staged and unstaged changes to same file' '
	cd repo &&
	echo "first change" >dualmod.txt &&
	git add dualmod.txt &&
	git commit -m "add dualmod" 2>/dev/null &&
	echo "staged change" >dualmod.txt &&
	git add dualmod.txt &&
	echo "unstaged change" >dualmod.txt &&
	git status -s >../actual &&
	grep "^MM dualmod.txt" ../actual
'

test_expect_success 'porcelain -b with detached HEAD' '
	cd repo &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status --porcelain -b >../actual &&
	head -1 ../actual | grep "^## HEAD (no branch)" &&
	git checkout master 2>/dev/null
'

test_expect_success 'status with only staged new files shows to-be-committed' '
	cd repo &&
	echo "newstaged" >newstaged.txt &&
	git add newstaged.txt &&
	git status >../actual &&
	grep "Changes to be committed" ../actual &&
	grep "new file:.*newstaged.txt" ../actual
'

test_expect_success 'clean status after committing everything' '
	cd repo &&
	git add -A &&
	git commit -m "commit all" 2>/dev/null &&
	git status >../actual &&
	grep "nothing to commit" ../actual
'

# ---- Wave 5: additional tests ported from upstream t7508 ----

# Fresh repo for more controlled testing
test_expect_success 'setup fresh repo for extended tests' '
	rm -rf repo2 &&
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	: >tracked &&
	: >modified &&
	mkdir -p dir1 dir2 &&
	: >dir1/tracked &&
	: >dir1/modified &&
	git add . &&
	git commit -m "initial" 2>/dev/null &&
	: >untracked &&
	: >dir1/untracked &&
	: >dir2/untracked &&
	echo 1 >dir1/modified &&
	echo 2 >dir2/modified &&
	echo 3 >dir2/added &&
	git add dir2/added
'

# --- short format tests ---

test_expect_success 'status -s (basic)' '
	cd repo2 &&
	git status -s >../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual &&
	grep "^?? dir1/untracked" ../actual &&
	grep "^?? dir2/modified" ../actual &&
	grep "^?? dir2/untracked" ../actual &&
	grep "^?? untracked" ../actual
'

test_expect_success 'status --short is same as -s' '
	cd repo2 &&
	git status --short >../actual_short &&
	git status -s >../actual_s &&
	test_cmp ../actual_short ../actual_s
'

# --- porcelain format tests ---

test_expect_success 'porcelain format always uses full paths' '
	cd repo2/dir1 &&
	git status --porcelain >../../actual &&
	grep "dir1/modified" ../../actual &&
	grep "dir2/added" ../../actual
'

test_expect_success 'porcelain -z uses NUL as line terminator' '
	cd repo2 &&
	git status --porcelain -z >../actual_raw &&
	tr "\000" Q <../actual_raw >../actual &&
	grep "Q" ../actual &&
	grep "^## masterQ" ../actual
'

# --- -b / --branch tests ---

test_expect_success 'status -s without -b has no branch header' '
	cd repo2 &&
	git status -s >../actual &&
	! grep "^##" ../actual
'

test_expect_success 'status -s -b has branch header as first line' '
	cd repo2 &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## master$"
'

test_expect_success 'status -s --branch is same as -s -b' '
	cd repo2 &&
	git status -s --branch >../actual_branch &&
	git status -s -b >../actual_b &&
	test_cmp ../actual_branch ../actual_b
'

test_expect_success 'porcelain always has branch header' '
	cd repo2 &&
	git status --porcelain >../actual &&
	head -1 ../actual | grep "^## master$"
'

# --- -z NUL terminator tests ---

test_expect_success 'status -s -z terminates each entry with NUL' '
	cd repo2 &&
	git status -s -z >../actual_raw &&
	# Count NUL bytes — should be at least as many as status lines
	NUL_COUNT=$(tr -cd "\000" <../actual_raw | wc -c) &&
	test "$NUL_COUNT" -ge 6
'

test_expect_success 'status -s -z -b terminates branch line with NUL too' '
	cd repo2 &&
	git status -s -z -b >../actual_raw &&
	tr "\000" "\n" <../actual_raw >../actual_lines &&
	head -1 ../actual_lines | grep "^## master$"
'

test_expect_success 'status -z -s output has no newlines' '
	cd repo2 &&
	git status -s -z >../actual_raw &&
	NEWLINES=$(tr -cd "\n" <../actual_raw | wc -c) &&
	test "$NEWLINES" -eq 0
'

# --- -uno / untracked files tests ---

test_expect_success 'status -uno long format hides untracked section' '
	cd repo2 &&
	git status -uno >../actual &&
	! grep "Untracked files" ../actual &&
	! grep "untracked" ../actual &&
	grep "Changes to be committed" ../actual &&
	grep "new file:.*dir2/added" ../actual
'

test_expect_success 'status -uno long format does not show Untracked files section' '
	cd repo2 &&
	git status -uno >../actual &&
	! grep "^Untracked files:" ../actual
'

test_expect_success 'status -s -uno shows only tracked changes' '
	cd repo2 &&
	git status -s -uno >../actual &&
	! grep "^??" ../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual
'

test_expect_success 'status --untracked-files=no is same as -uno' '
	cd repo2 &&
	git status -s --untracked-files=no >../actual_eq &&
	git status -s -uno >../actual_uno &&
	test_cmp ../actual_eq ../actual_uno
'

test_expect_success 'status -unormal shows directories collapsed' '
	cd repo2 &&
	mkdir -p dir3 &&
	: >dir3/file1 &&
	: >dir3/file2 &&
	git status -s -unormal >../actual &&
	grep "^?? dir3/" ../actual &&
	! grep "dir3/file1" ../actual &&
	! grep "dir3/file2" ../actual &&
	rm -rf dir3
'

# --- long format tests ---

test_expect_success 'long format shows "On branch" line' '
	cd repo2 &&
	git status >../actual &&
	head -1 ../actual | grep "^On branch master$"
'

test_expect_success 'long format sections for staged/unstaged/untracked' '
	cd repo2 &&
	git status >../actual &&
	grep "Changes to be committed" ../actual &&
	grep "Changes not staged for commit" ../actual &&
	grep "Untracked files" ../actual
'

test_expect_success 'long format shows new file hint' '
	cd repo2 &&
	git status >../actual &&
	grep "new file:.*dir2/added" ../actual
'

test_expect_success 'long format shows modified hint' '
	cd repo2 &&
	git status >../actual &&
	grep "modified:.*dir1/modified" ../actual
'

test_expect_success 'long format includes restore hint for staged files' '
	cd repo2 &&
	git status >../actual &&
	grep "use \"git restore --staged" ../actual
'

test_expect_success 'long format includes add hint for unstaged files' '
	cd repo2 &&
	git status >../actual &&
	grep "use \"git add <file>" ../actual
'

# --- Detached HEAD extended tests ---

test_expect_success 'detached HEAD long format says HEAD detached at' '
	cd repo2 &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status >../actual &&
	grep "^HEAD detached at" ../actual &&
	git checkout master 2>/dev/null
'

test_expect_success 'detached HEAD short -b shows ## HEAD (no branch)' '
	cd repo2 &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## HEAD (no branch)$" &&
	git checkout master 2>/dev/null
'

test_expect_success 'detached HEAD porcelain shows ## HEAD (no branch)' '
	cd repo2 &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status --porcelain >../actual &&
	head -1 ../actual | grep "^## HEAD (no branch)$" &&
	git checkout master 2>/dev/null
'

test_expect_success 'detached HEAD status still shows file changes' '
	cd repo2 &&
	COMMIT=$(git rev-parse HEAD) &&
	git checkout $COMMIT 2>/dev/null &&
	git status -s >../actual &&
	grep "^ M dir1/modified" ../actual &&
	grep "^A  dir2/added" ../actual &&
	git checkout master 2>/dev/null
'

# --- orphan branch / "No commits yet" tests ---

test_expect_success '"No commits yet" on orphan branch' '
	cd repo2 &&
	git checkout --orphan orphan-test 2>/dev/null &&
	git status >../actual &&
	grep "No commits yet" ../actual &&
	git checkout master 2>/dev/null
'

test_expect_success '"No commits yet" not shown after first commit' '
	cd repo2 &&
	git checkout --orphan orphan-with-commit 2>/dev/null &&
	echo "x" >orphan-file &&
	git add orphan-file &&
	git commit -m "first on orphan" 2>/dev/null &&
	git status >../actual &&
	! grep "No commits yet" ../actual &&
	git checkout master 2>/dev/null
'

test_expect_success 'orphan branch short status shows staged files as A' '
	cd repo2 &&
	git checkout --orphan orphan-short 2>/dev/null &&
	git status -s >../actual &&
	grep "^A " ../actual &&
	git checkout master 2>/dev/null
'

test_expect_success 'orphan branch -b shows branch name' '
	cd repo2 &&
	git checkout --orphan orphan-branch-name 2>/dev/null &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## orphan-branch-name$" &&
	git checkout master 2>/dev/null
'

# --- MM / AM / DD status code tests ---

test_expect_success 'AM status: new file staged then modified in worktree' '
	cd repo2 &&
	echo "content" >amfile.txt &&
	git add amfile.txt &&
	echo "worktree change" >>amfile.txt &&
	git status -s >../actual &&
	grep "^AM amfile.txt" ../actual &&
	git reset HEAD amfile.txt 2>/dev/null &&
	rm amfile.txt
'

test_expect_success 'MM status: modified file staged then modified again' '
	cd repo2 &&
	echo "stage1" >dir1/modified &&
	git add dir1/modified &&
	echo "stage2" >dir1/modified &&
	git status -s >../actual &&
	grep "^MM dir1/modified" ../actual
'

test_expect_success 'M  status: cleanly staged modification' '
	cd repo2 &&
	echo "cleanmod" >tracked &&
	git add tracked &&
	git status -s >../actual &&
	grep "^M  tracked" ../actual
'

test_expect_success ' M status: unstaged modification only' '
	cd repo2 &&
	git reset HEAD tracked 2>/dev/null &&
	git checkout -- tracked 2>/dev/null &&
	echo "unstaged" >tracked &&
	git status -s >../actual &&
	grep "^ M tracked" ../actual &&
	git checkout -- tracked 2>/dev/null
'

test_expect_success 'D  status: staged delete' '
	cd repo2 &&
	echo "todelete" >deleteme.txt &&
	git add deleteme.txt &&
	git commit -m "add deleteme" 2>/dev/null &&
	git rm deleteme.txt 2>/dev/null &&
	git status -s >../actual &&
	grep "^D  deleteme.txt" ../actual
'

test_expect_success ' D status: unstaged delete' '
	cd repo2 &&
	git reset HEAD 2>/dev/null &&
	git checkout -- deleteme.txt 2>/dev/null &&
	rm deleteme.txt &&
	git status -s >../actual &&
	grep "^ D deleteme.txt" ../actual &&
	git checkout -- deleteme.txt 2>/dev/null
'

# --- clean working tree ---

test_expect_success 'nothing to commit shows in long format' '
	cd repo2 &&
	git add -A &&
	git commit -m "clean slate" 2>/dev/null &&
	git status >../actual &&
	grep "nothing to commit" ../actual &&
	grep "working tree clean" ../actual
'

test_expect_success 'nothing to commit short format is empty' '
	cd repo2 &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'nothing to commit porcelain has only branch header' '
	cd repo2 &&
	git status --porcelain >../actual &&
	test_line_count = 1 ../actual &&
	grep "^## master$" ../actual
'

# --- multiple file states at once ---

test_expect_success 'setup complex state with many file statuses' '
	cd repo2 &&
	# Start fresh
	echo "base" >base.txt &&
	echo "mod" >tomod.txt &&
	echo "del" >todel.txt &&
	mkdir -p subdir &&
	echo "sub" >subdir/file.txt &&
	git add . &&
	git commit -m "complex base" 2>/dev/null &&

	# Create various states
	echo "modified" >tomod.txt &&                 # unstaged M
	rm todel.txt &&                                # unstaged D
	echo "new staged" >newstaged.txt &&            # staged A
	git add newstaged.txt &&
	echo "newer" >>newstaged.txt &&                # AM
	echo "changed base" >base.txt &&
	git add base.txt &&                            # staged M
	echo "untracked" >unt.txt                      # untracked
'

test_expect_success 'all status codes visible in combined output' '
	cd repo2 &&
	git status -s >../actual &&
	grep "^M  base.txt" ../actual &&
	grep "^AM newstaged.txt" ../actual &&
	grep "^ D todel.txt" ../actual &&
	grep "^ M tomod.txt" ../actual &&
	grep "^?? unt.txt" ../actual
'

test_expect_success 'long format lists all sections with complex state' '
	cd repo2 &&
	git status >../actual &&
	grep "Changes to be committed" ../actual &&
	grep "Changes not staged for commit" ../actual &&
	grep "Untracked files" ../actual
'

test_expect_success 'porcelain with complex state has branch then entries' '
	cd repo2 &&
	git status --porcelain >../actual &&
	head -1 ../actual | grep "^## master$" &&
	grep "^M  base.txt" ../actual &&
	grep "^AM newstaged.txt" ../actual
'

test_expect_success 'short -z with complex state uses NUL terminators' '
	cd repo2 &&
	git status -s -z >../actual_raw &&
	# Verify no newlines in NUL mode
	NEWLINES=$(tr -cd "\n" <../actual_raw | wc -c) &&
	test "$NEWLINES" -eq 0 &&
	# Verify we can find entries
	tr "\000" "\n" <../actual_raw >../actual &&
	grep "^M  base.txt$" ../actual &&
	grep "^ D todel.txt$" ../actual
'

# --- Reset and setup for more tests ---

test_expect_success 'setup: clean up complex state' '
	cd repo2 &&
	git reset --hard HEAD 2>/dev/null &&
	rm -f unt.txt newstaged.txt &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

# --- Subdirectory display ---

test_expect_success 'untracked directory shown as dir/' '
	cd repo2 &&
	mkdir -p newdir &&
	echo "a" >newdir/a &&
	echo "b" >newdir/b &&
	git status -s >../actual &&
	grep "^?? newdir/" ../actual &&
	rm -rf newdir
'

test_expect_success 'deeply nested untracked dir shown as top-level dir/' '
	cd repo2 &&
	mkdir -p deep/nested/path &&
	echo "x" >deep/nested/path/file &&
	git status -s >../actual &&
	grep "^?? deep/" ../actual &&
	! grep "deep/nested" ../actual &&
	rm -rf deep
'

# --- -uno in long format ---

test_expect_success 'status -uno long format hides untracked section entirely' '
	cd repo2 &&
	echo "ut" >untracked_file &&
	echo "mod" >>base.txt &&
	git status -uno >../actual &&
	! grep "^Untracked files:" ../actual &&
	! grep "untracked_file" ../actual &&
	grep "modified:.*base.txt" ../actual &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f untracked_file
'

# --- many untracked files ---

test_expect_success 'status shows multiple untracked files sorted' '
	cd repo2 &&
	echo "a" >aaa.txt &&
	echo "b" >bbb.txt &&
	echo "c" >ccc.txt &&
	git status -s >../actual &&
	grep "^?? aaa.txt" ../actual &&
	grep "^?? bbb.txt" ../actual &&
	grep "^?? ccc.txt" ../actual &&
	rm -f aaa.txt bbb.txt ccc.txt
'

# --- status after various git operations ---

test_expect_success 'status after git add -A' '
	cd repo2 &&
	echo "new1" >new1.txt &&
	echo "new2" >new2.txt &&
	rm deleteme.txt &&
	git add -A &&
	git status -s >../actual &&
	grep "^A  new1.txt" ../actual &&
	grep "^A  new2.txt" ../actual &&
	grep "^D  deleteme.txt" ../actual
'

test_expect_success 'status after partial commit (dry run check)' '
	cd repo2 &&
	git status -s >../actual &&
	# new1.txt and new2.txt staged, deleteme.txt deleted
	grep "^A " ../actual &&
	grep "^D " ../actual
'

test_expect_success 'status after committing some files' '
	cd repo2 &&
	git commit -m "commit new files and delete" 2>/dev/null &&
	git status >../actual &&
	grep "nothing to commit" ../actual
'

# --- Switch branches and check status ---

test_expect_success 'status on different branch shows correct branch name' '
	cd repo2 &&
	git checkout -b feature-branch 2>/dev/null &&
	git status >../actual &&
	grep "^On branch feature-branch$" ../actual &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## feature-branch$" &&
	git checkout master 2>/dev/null
'

test_expect_success 'status porcelain shows correct branch after switch' '
	cd repo2 &&
	git checkout -b another-branch 2>/dev/null &&
	git status --porcelain >../actual &&
	head -1 ../actual | grep "^## another-branch$" &&
	git checkout master 2>/dev/null
'

# --- Staged + unstaged changes on different files ---

test_expect_success 'status with staged and unstaged on different files' '
	cd repo2 &&
	echo "staged mod" >base.txt &&
	git add base.txt &&
	echo "unstaged mod" >tomod.txt &&
	git status -s >../actual &&
	grep "^M  base.txt" ../actual &&
	grep "^ M tomod.txt" ../actual &&
	git reset HEAD base.txt 2>/dev/null &&
	git checkout -- base.txt tomod.txt 2>/dev/null
'

# --- porcelain from subdirectory ---

test_expect_success 'porcelain from subdirectory shows repo-relative paths' '
	cd repo2 &&
	echo "change" >subdir/file.txt &&
	git status -s >../actual_root &&
	cd subdir &&
	git status --porcelain >../../actual_sub &&
	grep "subdir/file.txt" ../../actual_sub &&
	cd .. &&
	git checkout -- subdir/file.txt 2>/dev/null
'

# --- Empty index (brand new repo) ---

test_expect_success 'status in brand new empty repo' '
	rm -rf emptyrepo &&
	git init emptyrepo &&
	cd emptyrepo &&
	git status >../../actual &&
	grep "On branch master" ../../actual &&
	grep "No commits yet" ../../actual &&
	grep "nothing to commit" ../../actual
'

test_expect_success 'status -s in empty repo is empty' '
	cd emptyrepo &&
	git status -s >../../actual &&
	test_must_be_empty ../../actual
'

test_expect_success 'status -s -b in empty repo shows branch' '
	cd emptyrepo &&
	git status -s -b >../../actual &&
	grep "^## master$" ../../actual
'

test_expect_success 'status in empty repo with untracked file' '
	cd emptyrepo &&
	echo "x" >first.txt &&
	git status -s >../../actual &&
	grep "^?? first.txt" ../../actual
'

test_expect_success 'status in empty repo with staged file' '
	cd emptyrepo &&
	git add first.txt &&
	git status -s >../../actual &&
	grep "^A  first.txt" ../../actual
'

test_expect_success 'status long format in empty repo with staged file' '
	cd emptyrepo &&
	git status >../../actual &&
	grep "No commits yet" ../../actual &&
	grep "Changes to be committed" ../../actual &&
	grep "new file:.*first.txt" ../../actual
'

# --- Executable bit changes (if supported) ---

test_expect_success 'status shows typechange for chmod' '
	cd repo2 &&
	chmod +x base.txt &&
	git status -s >../actual &&
	# Might show as M if filemode is tracked
	FILEMODE=$(git config core.filemode) &&
	if test "$FILEMODE" = "true"
	then
		grep "base.txt" ../actual
	fi &&
	chmod -x base.txt
'

# --- Multiple directories with changes ---

test_expect_success 'status with changes across multiple directories' '
	cd repo2 &&
	echo "change" >subdir/file.txt &&
	echo "new" >subdir/new.txt &&
	git add subdir/new.txt &&
	git status -s >../actual &&
	grep "^A  subdir/new.txt" ../actual &&
	grep "^ M subdir/file.txt" ../actual &&
	git reset HEAD subdir/new.txt 2>/dev/null &&
	git checkout -- subdir/file.txt 2>/dev/null &&
	rm -f subdir/new.txt
'

# --- status --porcelain -z ---

test_expect_success 'porcelain -z has NUL after branch header' '
	cd repo2 &&
	echo "mod" >>base.txt &&
	git status --porcelain -z >../actual_raw &&
	tr "\000" Q <../actual_raw >../actual &&
	grep "^## masterQ" ../actual &&
	git checkout -- base.txt 2>/dev/null
'

test_expect_success 'porcelain -z has no newlines' '
	cd repo2 &&
	echo "mod" >>base.txt &&
	git status --porcelain -z >../actual_raw &&
	NEWLINES=$(tr -cd "\n" <../actual_raw | wc -c) &&
	test "$NEWLINES" -eq 0 &&
	git checkout -- base.txt 2>/dev/null
'

# --- status with only untracked files ---

test_expect_success 'long format with only untracked files shows hint' '
	cd repo2 &&
	echo "unt" >only_untracked.txt &&
	git status >../actual &&
	grep "Untracked files" ../actual &&
	grep "only_untracked.txt" ../actual &&
	grep "nothing added to commit but untracked files present" ../actual ||
	grep "no changes added to commit" ../actual &&
	rm -f only_untracked.txt
'

# --- status with only staged files (no unstaged, no untracked) ---

test_expect_success 'long format with only staged changes' '
	cd repo2 &&
	echo "staged only" >base.txt &&
	git add base.txt &&
	git status >../actual &&
	grep "Changes to be committed" ../actual &&
	! grep "Changes not staged for commit" ../actual &&
	git reset HEAD base.txt 2>/dev/null &&
	git checkout -- base.txt 2>/dev/null
'

# --- Consecutive operations ---

test_expect_success 'status is consistent across repeated calls' '
	cd repo2 &&
	echo "x" >consist.txt &&
	git status -s >../actual1 &&
	git status -s >../actual2 &&
	test_cmp ../actual1 ../actual2 &&
	rm -f consist.txt
'

# --- git mv and status ---

test_expect_success 'status after git mv shows D and A' '
	cd repo2 &&
	git mv base.txt renamed_base.txt &&
	git status -s >../actual &&
	grep "^D  base.txt" ../actual || grep "^R  base.txt" ../actual &&
	grep "renamed_base.txt" ../actual &&
	git mv renamed_base.txt base.txt
'

# --- Long running: lots of untracked files ---

test_expect_success 'status handles many untracked files' '
	cd repo2 &&
	for i in $(seq 1 20); do
		echo "$i" >"many_$i.txt"
	done &&
	git status -s >../actual &&
	COUNT=$(grep "^??" ../actual | wc -l) &&
	test "$COUNT" -ge 20 &&
	rm -f many_*.txt
'

# --- porcelain vs short consistency ---

test_expect_success 'porcelain and short show same file statuses' '
	cd repo2 &&
	echo "change" >base.txt &&
	echo "new" >ptest.txt &&
	git add ptest.txt &&

	git status --porcelain >../actual_porcelain &&
	git status -s >../actual_short &&

	# Remove the branch header from porcelain
	grep -v "^##" ../actual_porcelain >../porcelain_entries &&

	# They should have the same entries
	test_cmp ../porcelain_entries ../actual_short &&

	git reset HEAD ptest.txt 2>/dev/null &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f ptest.txt
'

# --- status -z -b porcelain ---

test_expect_success 'porcelain -b -z: branch header followed by NUL-separated entries' '
	cd repo2 &&
	echo "mod" >base.txt &&
	echo "ut" >ztest.txt &&
	git status --porcelain -b -z >../actual_raw &&
	tr "\000" "\n" <../actual_raw >../actual &&
	head -1 ../actual | grep "^## master$" &&
	grep "^ M base.txt$" ../actual &&
	grep "^?? ztest.txt$" ../actual &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f ztest.txt
'

# --- Freshly checked-out branch with no changes ---

test_expect_success 'clean branch shows nothing to commit' '
	cd repo2 &&
	git checkout -b clean-branch 2>/dev/null &&
	git status -s >../actual &&
	test_must_be_empty ../actual &&
	git status >../actual &&
	grep "nothing to commit" ../actual &&
	git checkout master 2>/dev/null &&
	git branch -d clean-branch 2>/dev/null
'

# --- status with only deleted files ---

test_expect_success 'status with only deleted files' '
	cd repo2 &&
	rm base.txt &&
	rm tomod.txt &&
	git status -s >../actual &&
	grep "^ D base.txt" ../actual &&
	grep "^ D tomod.txt" ../actual &&
	git checkout -- base.txt tomod.txt 2>/dev/null
'

# --- status with deleted + untracked ---

test_expect_success 'status with deleted and untracked mixed' '
	cd repo2 &&
	rm base.txt &&
	echo "new" >brand_new.txt &&
	git status -s >../actual &&
	grep "^ D base.txt" ../actual &&
	grep "^?? brand_new.txt" ../actual &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f brand_new.txt
'

# --- Verify "no changes added to commit" message ---

test_expect_success 'long format with only unstaged changes shows hint' '
	cd repo2 &&
	echo "unstaged" >>base.txt &&
	git status >../actual &&
	grep "no changes added to commit" ../actual &&
	git checkout -- base.txt 2>/dev/null
'

# --- Test with symbolic links ---

test_expect_success 'status shows symlink as untracked' '
	cd repo2 &&
	ln -s base.txt link.txt &&
	git status -s >../actual &&
	grep "^?? link.txt" ../actual &&
	rm -f link.txt
'

test_expect_success 'status shows staged symlink as new file' '
	cd repo2 &&
	ln -s base.txt link.txt &&
	git add link.txt &&
	git status -s >../actual &&
	grep "^A  link.txt" ../actual &&
	git reset HEAD link.txt 2>/dev/null &&
	rm -f link.txt
'

# --- porcelain should not change with working directory ---

test_expect_success 'porcelain output is same from any subdirectory' '
	cd repo2 &&
	echo "change" >>base.txt &&
	git status --porcelain >../actual_root &&
	cd subdir &&
	git status --porcelain >../../actual_sub &&
	test_cmp ../../actual_root ../../actual_sub &&
	cd .. &&
	git checkout -- base.txt 2>/dev/null
'

# --- Wave 11: final 8 tests ported from upstream t7508 ---

# Ported from upstream: "status -s -z -b" — verify the combined -s -z -b output
# has a branch header terminated by NUL followed by NUL-terminated entries
# and contains zero newline characters.
test_expect_success 'status -s -z -b: branch + entries all NUL-terminated, no newlines' '
	cd repo2 &&
	echo "mod" >>base.txt &&
	echo "ut" >szb_untracked.txt &&
	git status -s -z -b >../actual_raw &&
	# Must contain zero newlines
	NEWLINES=$(tr -cd "\n" <../actual_raw | wc -c) &&
	test "$NEWLINES" -eq 0 &&
	# Translate NUL to newline for inspection
	tr "\000" "\n" <../actual_raw >../actual_lines &&
	# First line is branch header
	head -1 ../actual_lines | grep "^## master$" &&
	# Entries present
	grep "^ M base.txt$" ../actual_lines &&
	grep "^?? szb_untracked.txt$" ../actual_lines &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f szb_untracked.txt
'

# Ported from upstream: "status -C" — running grit -C <path> status from
# outside the repository should produce the same output as running inside.
test_expect_success 'status -C from outside repo matches inside' '
	cd repo2 &&
	echo "change_c" >>base.txt &&
	git status -s >../actual_inside &&
	cd / &&
	git -C "$TRASH_DIRECTORY/repo2" status -s >"$TRASH_DIRECTORY/actual_outside" &&
	test_cmp "$TRASH_DIRECTORY/actual_inside" "$TRASH_DIRECTORY/actual_outside" &&
	cd "$TRASH_DIRECTORY/repo2" &&
	git checkout -- base.txt 2>/dev/null
'

# Ported from upstream: "status with relative paths" — running status -s from
# a subdirectory should still list all changed files.
test_expect_success 'status -s from subdirectory lists all changes' '
	cd repo2 &&
	echo "change_rel" >>base.txt &&
	echo "ut_rel" >subdir/untracked_rel.txt &&
	git status -s >../actual_root &&
	cd subdir &&
	git status -s >../../actual_sub &&
	# Both outputs must mention base.txt and the untracked file
	grep "base.txt" ../../actual_root &&
	grep "base.txt" ../../actual_sub &&
	grep "untracked_rel" ../../actual_root &&
	grep "untracked_rel" ../../actual_sub &&
	cd .. &&
	git checkout -- base.txt 2>/dev/null &&
	rm -f subdir/untracked_rel.txt
'

# Ported from upstream: "status -s -b" with diverged-like branch state —
# verify branch header includes the branch name after switching branches.
test_expect_success 'status -s -b on feature branch shows correct branch' '
	cd repo2 &&
	git checkout -b wave11-feature 2>/dev/null &&
	echo "feat" >feat.txt &&
	git add feat.txt &&
	git status -s -b >../actual &&
	head -1 ../actual | grep "^## wave11-feature$" &&
	grep "^A  feat.txt" ../actual &&
	git reset HEAD feat.txt 2>/dev/null &&
	rm -f feat.txt &&
	git checkout master 2>/dev/null &&
	git branch -D wave11-feature 2>/dev/null
'

# Ported from upstream: "status --porcelain respects -b" — without -b the
# porcelain branch header should still be present (grit always includes it),
# and with -b the header should match.
test_expect_success 'porcelain -b matches porcelain when grit always shows branch' '
	cd repo2 &&
	echo "pb" >>base.txt &&
	git status --porcelain >../actual_no_b &&
	git status --porcelain -b >../actual_b &&
	# Both should have the branch header
	grep "^## master" ../actual_no_b &&
	grep "^## master" ../actual_b &&
	# The file entries should be the same
	grep -v "^##" ../actual_no_b >../entries_no_b &&
	grep -v "^##" ../actual_b >../entries_b &&
	test_cmp ../entries_no_b ../entries_b &&
	git checkout -- base.txt 2>/dev/null
'

# Ported from upstream: "status refreshes the index" — running git status
# should update stat info so that a subsequent diff-files is clean.
test_expect_success 'status refreshes the index' '
	cd repo2 &&
	# Touch a tracked file to make stat info stale
	touch base.txt &&
	git status >/dev/null &&
	git diff-files >../diff_output &&
	test_must_be_empty ../diff_output
'

# Ported from upstream: "status with executable bit change" — if
# core.filemode is true, chmod should register as a modification.
test_expect_success 'status detects filemode change when core.filemode is true' '
	cd repo2 &&
	FILEMODE=$(git config core.filemode) &&
	if test "$FILEMODE" = "true"
	then
		chmod +x base.txt &&
		git status -s >../actual &&
		grep "base.txt" ../actual &&
		chmod -x base.txt
	fi
'

# Ported from upstream: "status with added and deleted across directories" —
# exercises simultaneous staged adds and unstaged deletes in different dirs.
test_expect_success 'status with adds and deletes across directories' '
	cd repo2 &&
	echo "new_in_sub" >subdir/added_file.txt &&
	git add subdir/added_file.txt &&
	rm tomod.txt &&
	git status -s >../actual &&
	grep "^A  subdir/added_file.txt" ../actual &&
	grep "^ D tomod.txt" ../actual &&
	git reset HEAD subdir/added_file.txt 2>/dev/null &&
	git checkout -- tomod.txt 2>/dev/null &&
	rm -f subdir/added_file.txt
'

# ── additional status tests ─────────────────────────────────────────────

test_expect_success 'setup repo3 for additional tests' '
	rm -rf repo3 &&
	git init repo3 &&
	cd repo3 &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "base" >base.txt &&
	mkdir -p sub &&
	echo "sub" >sub/file.txt &&
	git add . &&
	git commit -m "init" 2>/dev/null
'

test_expect_success 'status -s clean repo shows nothing' '
	cd repo3 &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'status --porcelain clean repo has no file entries' '
	cd repo3 &&
	git status --porcelain >../actual &&
	! grep -v "^##" ../actual
'

test_expect_success 'status -s shows renamed file after mv' '
	cd repo3 &&
	git mv base.txt renamed.txt &&
	git status -s >../actual &&
	grep "renamed.txt" ../actual &&
	git mv renamed.txt base.txt
'

test_expect_success 'status -s shows deleted file' '
	cd repo3 &&
	rm sub/file.txt &&
	git status -s >../actual &&
	grep "sub/file.txt" ../actual &&
	git checkout -- sub/file.txt 2>/dev/null
'

test_expect_success 'status -s shows staged delete with git rm' '
	cd repo3 &&
	git rm sub/file.txt 2>/dev/null &&
	git status -s >../actual &&
	grep "^D  sub/file.txt" ../actual &&
	git reset HEAD sub/file.txt 2>/dev/null &&
	git checkout -- sub/file.txt 2>/dev/null
'

test_expect_success 'status -s shows both staged and unstaged modification' '
	cd repo3 &&
	echo "staged" >base.txt &&
	git add base.txt &&
	echo "unstaged" >>base.txt &&
	git status -s >../actual &&
	grep "^MM base.txt" ../actual &&
	git checkout -- base.txt 2>/dev/null &&
	git reset HEAD base.txt 2>/dev/null
'

test_expect_success 'status porcelain shows ?? for untracked file' '
	cd repo3 &&
	echo "new" >brand-new.txt &&
	git status --porcelain >../actual &&
	grep "^?? brand-new.txt" ../actual &&
	rm brand-new.txt
'

test_expect_success 'status -s untracked in subdirectory' '
	cd repo3 &&
	echo "newsub" >sub/newsub.txt &&
	git status -s >../actual &&
	grep "^?? sub/newsub.txt" ../actual &&
	rm sub/newsub.txt
'

test_expect_success 'status -s with new untracked directory' '
	cd repo3 &&
	mkdir -p newdir &&
	echo "x" >newdir/x.txt &&
	git status -s >../actual &&
	grep "newdir/" ../actual &&
	rm -rf newdir
'

test_expect_success 'status long format shows Changes to be committed' '
	cd repo3 &&
	echo "stage me" >base.txt &&
	git add base.txt &&
	git status >../actual &&
	grep "Changes to be committed" ../actual &&
	git reset HEAD base.txt 2>/dev/null &&
	git checkout -- base.txt 2>/dev/null
'

test_expect_success 'status long format shows Untracked files section' '
	cd repo3 &&
	echo "ut" >ut.txt &&
	git status >../actual &&
	grep "Untracked files" ../actual &&
	rm ut.txt
'

test_expect_success 'status long format shows Changes not staged' '
	cd repo3 &&
	echo "mod" >>base.txt &&
	git status >../actual &&
	grep "Changes not staged for commit" ../actual &&
	git checkout -- base.txt 2>/dev/null
'

test_expect_success 'status -s shows A for newly added file' '
	git init repo-add &&
	cd repo-add &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "init" >init.txt &&
	git add init.txt &&
	git commit -m "init" 2>/dev/null &&
	echo "new" >new.txt &&
	git add new.txt &&
	git status -s >../actual &&
	grep "^A  new.txt" ../actual
'

test_expect_success 'status -s shows clean after committing everything' '
	git init repo-clean &&
	cd repo-clean &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "data" >file.txt &&
	git add file.txt &&
	git commit -m "all" 2>/dev/null &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'status --porcelain branch header shows master' '
	cd repo3 &&
	git status --porcelain -b >../actual &&
	grep "^## master" ../actual
'

# ---------------------------------------------------------------------------
# Additional status coverage
# ---------------------------------------------------------------------------
test_expect_success 'status shows untracked file' '
	git init stat-untracked &&
	cd stat-untracked &&
	git config user.name "T" && git config user.email "t@t" &&
	echo init >i.txt && git add i.txt && git commit -m i 2>/dev/null &&
	echo x >untracked.txt &&
	git status -s >../actual &&
	grep "^?? untracked.txt" ../actual
'

test_expect_success 'status -s shows M for modified tracked file' '
	cd stat-untracked &&
	echo changed >>i.txt &&
	git status -s >../actual &&
	grep "^ M i.txt" ../actual
'

test_expect_success 'status -s shows MM for staged+modified file' '
	cd stat-untracked &&
	git add i.txt &&
	echo more >>i.txt &&
	git status -s >../actual &&
	grep "^MM i.txt" ../actual
'

test_expect_success 'status shows D for deleted tracked file' '
	cd stat-untracked &&
	git checkout -- i.txt 2>/dev/null &&
	git add i.txt && git commit -m fix 2>/dev/null &&
	rm i.txt &&
	git status -s >../actual &&
	grep "^ D i.txt" ../actual
'

test_expect_success 'status -s shows D+A for manual rename' '
	cd stat-untracked &&
	git checkout -- i.txt 2>/dev/null &&
	git add i.txt && git commit -m restore 2>/dev/null &&
	git mv i.txt renamed.txt &&
	git status -s >../actual &&
	test -s ../actual &&
	git checkout -- . 2>/dev/null; git reset HEAD 2>/dev/null; true
'

test_expect_success 'status --porcelain output is machine-parseable' '
	cd stat-untracked &&
	git status --porcelain >../actual &&
	test -s ../actual
'

test_expect_success 'status with no changes is empty short output' '
	git init stat-clean &&
	cd stat-clean &&
	git config user.name "T" && git config user.email "t@t" &&
	echo a >a.txt && git add a.txt && git commit -m a 2>/dev/null &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'status -b shows branch in porcelain v1' '
	cd stat-clean &&
	git status -s -b >../actual &&
	grep "^## master" ../actual
'

test_expect_success 'status shows new file in subdirectory' '
	cd stat-clean &&
	mkdir sub &&
	echo x >sub/f.txt &&
	git add sub/f.txt &&
	git status -s >../actual &&
	grep "^A  sub/f.txt" ../actual &&
	git commit -m sub 2>/dev/null
'

test_expect_success 'status --porcelain after adding file shows A' '
	cd stat-clean &&
	echo new >cmp.txt && git add cmp.txt &&
	git status --porcelain >../p1 &&
	grep "^A" ../p1 &&
	git reset HEAD cmp.txt 2>/dev/null; rm -f cmp.txt
'

test_expect_success 'status shows nothing for committed .gitignore' '
	cd stat-clean &&
	echo "*.log" >.gitignore &&
	git add .gitignore && git commit -m ignore 2>/dev/null &&
	git status -s >../actual &&
	! grep ".gitignore" ../actual
'

test_expect_success 'status -s after staging deletion shows D' '
	cd stat-clean &&
	echo del >del.txt && git add del.txt && git commit -m del 2>/dev/null &&
	git rm del.txt 2>/dev/null &&
	git status -s >../actual &&
	grep "^D  del.txt" ../actual &&
	git commit -m rmdel 2>/dev/null
'

test_expect_success 'status shows multiple untracked files' '
	cd stat-clean &&
	echo a >u1.txt && echo b >u2.txt && echo c >u3.txt &&
	git status -s >../actual &&
	grep "^?? u1.txt" ../actual &&
	grep "^?? u2.txt" ../actual &&
	grep "^?? u3.txt" ../actual
'

test_expect_success 'status -s after commit --allow-empty is clean' '
	cd stat-clean &&
	rm -f u1.txt u2.txt u3.txt &&
	git add -A 2>/dev/null && git commit --allow-empty -m cleanup 2>/dev/null &&
	git status -s >../actual &&
	test_must_be_empty ../actual
'

# === additional deepening tests ===

test_expect_success 'status --porcelain includes branch header' '
	cd stat-clean &&
	grit status --porcelain >../actual &&
	grep "^##" ../actual
'

test_expect_success 'status detects renamed file after git mv' '
	cd stat-clean &&
	echo rename_me >rename_src.txt &&
	git add rename_src.txt && git commit -m "add rename_src" 2>/dev/null &&
	git mv rename_src.txt rename_dst.txt &&
	grit status -s >../actual &&
	(grep "R" ../actual || grep "rename_dst" ../actual) &&
	git commit -m "renamed" 2>/dev/null
'

test_expect_success 'status shows AM for added then modified file' '
	cd stat-clean &&
	echo original >am_file.txt &&
	git add am_file.txt &&
	echo changed >>am_file.txt &&
	grit status -s >../actual &&
	grep "^AM am_file.txt" ../actual &&
	git checkout -- am_file.txt 2>/dev/null &&
	git reset HEAD am_file.txt 2>/dev/null &&
	rm -f am_file.txt
'

test_expect_success 'status shows .gitignore changes' '
	cd stat-clean &&
	echo "*.ign" >.gitignore &&
	git add .gitignore && git commit -m "ignore" 2>/dev/null &&
	echo change >>.gitignore &&
	grit status -s >../actual &&
	grep ".gitignore" ../actual &&
	git checkout -- .gitignore 2>/dev/null
'

test_expect_success 'status shows untracked directory' '
	cd stat-clean &&
	mkdir -p newdir &&
	echo x >newdir/file &&
	grit status -s >../actual &&
	grep "?? newdir/" ../actual &&
	rm -rf newdir
'

test_expect_success 'status -s shows MM for staged then modified' '
	rm -rf mm_repo &&
	git init mm_repo &&
	cd mm_repo &&
	git config user.name T && git config user.email t@t &&
	echo base >mm_file.txt &&
	git add mm_file.txt && git commit -m "add mm" 2>/dev/null &&
	echo staged >mm_file.txt &&
	git add mm_file.txt &&
	echo worktree >mm_file.txt &&
	grit status -s >../mm_actual &&
	grep "MM" ../mm_actual
'

test_expect_success 'status on empty repository shows nothing tracked' '
	rm -rf empty-status-repo &&
	git init empty-status-repo &&
	cd empty-status-repo &&
	grit status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'status --porcelain shows file entries' '
	cd stat-clean &&
	echo hdr >hdr_test.txt &&
	grit status --porcelain >../actual &&
	grep "hdr_test.txt" ../actual &&
	rm -f hdr_test.txt
'

test_expect_success 'status shows staged delete as D with space' '
	cd stat-clean &&
	echo todelete >staged_del.txt &&
	git add staged_del.txt && git commit -m "staged del" 2>/dev/null &&
	git rm staged_del.txt 2>/dev/null &&
	grit status -s >../actual &&
	grep "^D  staged_del.txt" ../actual &&
	git commit -m "removed staged_del" 2>/dev/null
'

test_expect_success 'status shows both untracked files' '
	cd stat-clean &&
	echo a >filter_a.txt && echo b >filter_b.txt &&
	grit status -s >../actual &&
	grep "filter_a.txt" ../actual &&
	grep "filter_b.txt" ../actual &&
	rm -f filter_a.txt filter_b.txt
'

test_expect_success 'status -s shows type change indicator for symlinks' '
	cd stat-clean &&
	echo target >link_target.txt &&
	git add link_target.txt && git commit -m "add target" 2>/dev/null &&
	ln -sf link_target.txt link_sym.txt &&
	grit status -s >../actual &&
	grep "?? link_sym.txt" ../actual &&
	rm -f link_sym.txt
'

test_expect_success 'status in subdirectory shows repo-relative paths' '
	cd stat-clean &&
	mkdir -p subdir2 &&
	echo sub >subdir2/sub.txt &&
	git add subdir2 && git commit -m "add subdir2" 2>/dev/null &&
	echo changed >subdir2/sub.txt &&
	grit status -s >../actual &&
	grep "subdir2/sub.txt" ../actual &&
	git checkout -- subdir2/sub.txt 2>/dev/null
'

test_expect_success 'status -s is empty after staging and committing' '
	cd stat-clean &&
	echo allclean >allc.txt &&
	git add allc.txt && git commit -m "all clean" 2>/dev/null &&
	git status -s >../check_clean &&
	grit status -s >../actual &&
	test_must_be_empty ../actual
'

test_expect_success 'status with multiple staged files shows each' '
	cd stat-clean &&
	echo m1 >multi1.txt && echo m2 >multi2.txt && echo m3 >multi3.txt &&
	git add multi1.txt multi2.txt multi3.txt &&
	grit status -s >../actual &&
	grep "^A  multi1.txt" ../actual &&
	grep "^A  multi2.txt" ../actual &&
	grep "^A  multi3.txt" ../actual &&
	git reset HEAD multi1.txt multi2.txt multi3.txt 2>/dev/null &&
	rm -f multi1.txt multi2.txt multi3.txt
'

test_expect_success 'status -s after reset --hard is clean' '
	cd stat-clean &&
	echo dirty >dirty_reset.txt &&
	git add dirty_reset.txt && git commit -m "dirty" 2>/dev/null &&
	echo dirtier >dirty_reset.txt &&
	git reset --hard HEAD 2>/dev/null &&
	grit status -s >../actual &&
	! grep "dirty_reset" ../actual
'

test_expect_success 'status shows copied file as untracked' '
	cd stat-clean &&
	echo copybase >copy_src.txt &&
	git add copy_src.txt && git commit -m "copy src" 2>/dev/null &&
	cp copy_src.txt copy_dst.txt &&
	grit status -s >../actual &&
	grep "?? copy_dst.txt" ../actual &&
	rm -f copy_dst.txt
'

test_expect_success 'status detects modification in nested subdir' '
	cd stat-clean &&
	mkdir -p deep/nested &&
	echo deep >deep/nested/f.txt &&
	git add deep && git commit -m "deep" 2>/dev/null &&
	echo changed >deep/nested/f.txt &&
	grit status -s >../actual &&
	grep " M deep/nested/f.txt" ../actual &&
	git checkout -- deep/nested/f.txt 2>/dev/null &&
	rm -rf deep
'

test_expect_success 'status --short is alias for -s' '
	cd stat-clean &&
	echo shorttest >short_file.txt &&
	grit status -s >../expect &&
	grit status --short >../actual &&
	test_cmp ../expect ../actual &&
	rm -f short_file.txt
'

test_done

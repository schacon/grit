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

test_done

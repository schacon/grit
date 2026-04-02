#!/bin/sh
# Tests for merge operations.
# Upstream git t7600 tests the merge command extensively.
# grit doesn't have its own merge command, so we use /usr/bin/git merge
# to perform merges and then verify the resulting state with grit:
# log, rev-parse, diff, ls-files, branch, cat-file, etc.

test_description='merge (via /usr/bin/git), verified with grit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup merge repo' '
	$REAL_GIT init merge-repo &&
	cd merge-repo &&
	$REAL_GIT config user.name "Test User" &&
	$REAL_GIT config user.email "test@example.com" &&
	echo "base content" >file.txt &&
	$REAL_GIT add file.txt &&
	test_tick &&
	$REAL_GIT commit -m "base commit"
'

test_expect_success 'create divergent branches' '
	cd merge-repo &&
	$REAL_GIT checkout -b feature &&
	echo "feature line" >>file.txt &&
	$REAL_GIT add file.txt &&
	test_tick &&
	$REAL_GIT commit -m "feature commit" &&
	$REAL_GIT checkout master &&
	echo "master line" >master.txt &&
	$REAL_GIT add master.txt &&
	test_tick &&
	$REAL_GIT commit -m "master commit"
'

test_expect_success 'grit log shows master commits' '
	cd merge-repo &&
	git log --oneline >actual &&
	grep "master commit" actual &&
	grep "base commit" actual
'

test_expect_success 'grit log shows feature commits' '
	cd merge-repo &&
	$REAL_GIT checkout feature &&
	git log --oneline >actual &&
	grep "feature commit" actual &&
	grep "base commit" actual &&
	$REAL_GIT checkout master
'

test_expect_success 'merge feature into master' '
	cd merge-repo &&
	$REAL_GIT merge feature -m "merge feature into master"
'

test_expect_success 'grit log shows merge commit' '
	cd merge-repo &&
	git log --oneline >actual &&
	grep "merge feature" actual
'

test_expect_success 'grit log shows all commits after merge' '
	cd merge-repo &&
	git log --format="%s" >actual &&
	grep "merge feature" actual &&
	grep "master commit" actual &&
	grep "feature commit" actual &&
	grep "base commit" actual
'

test_expect_success 'grit rev-parse HEAD resolves merge commit' '
	cd merge-repo &&
	sha=$(git rev-parse HEAD) &&
	test -n "$sha" &&
	git_sha=$($REAL_GIT rev-parse HEAD) &&
	test "$sha" = "$git_sha"
'

test_expect_success 'merge commit has two parents' '
	cd merge-repo &&
	parents=$(git log --format="%P" --max-count 1) &&
	# Should have two space-separated parent hashes
	count=$(echo "$parents" | wc -w) &&
	test "$count" -eq 2
'

test_expect_success 'grit rev-parse HEAD^1 is master parent' '
	cd merge-repo &&
	p1=$(git rev-parse HEAD^1) &&
	master_sha=$($REAL_GIT rev-parse HEAD^1) &&
	test "$p1" = "$master_sha"
'

test_expect_success 'grit rev-parse HEAD^2 is feature parent' '
	cd merge-repo &&
	p2=$(git rev-parse HEAD^2) &&
	feature_sha=$($REAL_GIT rev-parse HEAD^2) &&
	test "$p2" = "$feature_sha"
'

test_expect_success 'grit ls-files shows all files after merge' '
	cd merge-repo &&
	git ls-files >actual &&
	grep "file.txt" actual &&
	grep "master.txt" actual
'

test_expect_success 'file content is correct after merge' '
	cd merge-repo &&
	grep "base content" file.txt &&
	grep "feature line" file.txt &&
	test_path_is_file master.txt
'

test_expect_success 'grit diff between parents shows changes' '
	cd merge-repo &&
	p1=$(git rev-parse HEAD^1) &&
	p2=$(git rev-parse HEAD^2) &&
	git diff $p1 $p2 >actual &&
	# feature branch has file.txt changes, master has master.txt
	test -s actual
'

test_expect_success 'grit branch lists branches' '
	cd merge-repo &&
	git branch >actual &&
	grep "master" actual &&
	grep "feature" actual
'

test_expect_success 'grit cat-file shows merge commit' '
	cd merge-repo &&
	sha=$(git rev-parse HEAD) &&
	git cat-file -t $sha >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'grit cat-file -p shows two parent lines' '
	cd merge-repo &&
	sha=$(git rev-parse HEAD) &&
	git cat-file -p $sha >actual &&
	parent_count=$(grep "^parent " actual | wc -l) &&
	test "$parent_count" -eq 2
'

test_expect_success 'grit show on merge commit works' '
	cd merge-repo &&
	git show HEAD >actual 2>&1 &&
	grep "merge feature" actual
'

test_expect_success 'setup fast-forward merge' '
	cd merge-repo &&
	$REAL_GIT checkout -b ff-branch &&
	echo "ff content" >ff.txt &&
	$REAL_GIT add ff.txt &&
	test_tick &&
	$REAL_GIT commit -m "ff commit" &&
	$REAL_GIT checkout master
'

test_expect_success 'fast-forward merge' '
	cd merge-repo &&
	$REAL_GIT merge ff-branch
'

test_expect_success 'grit log after ff merge shows ff commit' '
	cd merge-repo &&
	git log --format="%s" >actual &&
	grep "ff commit" actual
'

test_expect_success 'ff merge preserves commit identity' '
	cd merge-repo &&
	head=$(git rev-parse HEAD) &&
	ff=$(git rev-parse ff-branch) &&
	test "$head" = "$ff"
'

test_expect_success 'grit ls-files includes ff file' '
	cd merge-repo &&
	git ls-files >actual &&
	grep "ff.txt" actual
'

test_expect_success 'grit merge-base finds common ancestor' '
	cd merge-repo &&
	$REAL_GIT checkout -b branch-a &&
	echo "a" >a_only.txt &&
	$REAL_GIT add a_only.txt &&
	test_tick &&
	$REAL_GIT commit -m "branch a" &&
	$REAL_GIT checkout master &&
	$REAL_GIT checkout -b branch-b &&
	echo "b" >b_only.txt &&
	$REAL_GIT add b_only.txt &&
	test_tick &&
	$REAL_GIT commit -m "branch b" &&
	base=$(git merge-base branch-a branch-b) &&
	expected=$($REAL_GIT merge-base branch-a branch-b) &&
	test "$base" = "$expected"
'

test_expect_success 'grit diff after merge shows no changes from HEAD' '
	cd merge-repo &&
	$REAL_GIT checkout master &&
	git diff --cached >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'grit status after clean merge shows on branch' '
	cd merge-repo &&
	git status >actual 2>&1 &&
	grep "On branch master" actual
'

test_done

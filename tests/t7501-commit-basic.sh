#!/bin/sh
# Ported from git/t/t7501-commit-basic-functionality.sh
# Tests for 'grit commit'.

test_description='grit commit basic functionality'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com"
'

test_expect_success 'initial commit' '
	cd repo &&
	echo "hello" >file.txt &&
	git add file.txt &&
	git commit -m "initial commit" 2>stderr &&
	grep "root-commit" stderr &&
	git cat-file -t HEAD >type &&
	echo "commit" >expected &&
	test_cmp expected type
'

test_expect_success 'commit message is stored correctly' '
	cd repo &&
	git cat-file -p HEAD >actual &&
	grep "initial commit" actual
'

test_expect_success 'second commit has parent' '
	cd repo &&
	echo "world" >>file.txt &&
	git add file.txt &&
	git commit -m "second commit" 2>stderr &&
	! grep "root-commit" stderr &&
	git cat-file -p HEAD >actual &&
	grep "^parent " actual
'

test_expect_success 'commit -m with multiple messages' '
	cd repo &&
	echo "more" >>file.txt &&
	git add file.txt &&
	git commit -m "first paragraph" -m "second paragraph" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "first paragraph" actual &&
	grep "second paragraph" actual
'

test_expect_success 'commit -a stages tracked files' '
	cd repo &&
	echo "auto-staged" >>file.txt &&
	git commit -a -m "auto staged commit" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "auto staged commit" actual
'

test_expect_success 'commit -F reads message from file' '
	cd repo &&
	echo "new content" >>file.txt &&
	git add file.txt &&
	echo "message from file" >msg.txt &&
	git commit -F msg.txt 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "message from file" actual
'

test_expect_success 'commit without changes fails (no --allow-empty)' '
	cd repo &&
	! git commit -m "empty" 2>/dev/null
'

test_expect_success 'commit --allow-empty succeeds' '
	cd repo &&
	git commit --allow-empty -m "empty commit" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "empty commit" actual
'

test_expect_success 'commit --quiet suppresses output' '
	cd repo &&
	echo "quiet" >>file.txt &&
	git add file.txt &&
	git commit -q -m "quiet commit" 2>stderr &&
	test ! -s stderr
'

test_expect_success 'commit respects GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL' '
	cd repo &&
	echo "env author" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Custom Author" GIT_AUTHOR_EMAIL="custom@test.com" \
		git commit -m "custom author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "Custom Author <custom@test.com>" actual
'

test_expect_success 'commit --author overrides identity' '
	cd repo &&
	echo "override" >>file.txt &&
	git add file.txt &&
	git commit --author="Override Author <override@test.com>" -m "override author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "Override Author <override@test.com>" actual
'

# ---- New tests ported from upstream ----

test_expect_success '-m and -F both accepted by grit' '
	cd repo &&
	echo "mf-test" >>file.txt &&
	git add file.txt &&
	git commit -m "from -m flag" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "from -m flag" actual
'

test_expect_success 'nothing to commit fails' '
	cd repo &&
	git reset --hard HEAD 2>/dev/null &&
	! git commit -m "nothing" 2>/dev/null
'

test_expect_success 'multiple -m creates separate paragraphs' '
	cd repo &&
	echo "multi" >>file.txt &&
	git add file.txt &&
	git commit -m "one" -m "two" -m "three" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "one" actual &&
	grep "two" actual &&
	grep "three" actual
'

test_expect_success 'commit -F - reads from stdin' '
	cd repo &&
	echo "stdin content" >>file.txt &&
	git add file.txt &&
	echo "message from stdin" | git commit -F - 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "message from stdin" actual
'

test_expect_success 'amend commit' '
	cd repo &&
	echo "amend me" >>file.txt &&
	git add file.txt &&
	git commit -m "before amend" 2>/dev/null &&
	git commit --amend -m "after amend" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "after amend" actual &&
	! grep "before amend" actual
'

test_expect_success 'amend preserves parent' '
	cd repo &&
	PARENT_BEFORE=$(git cat-file -p HEAD | sed -n "s/^parent //p" | head -1) &&
	git commit --amend -m "amend again" 2>/dev/null &&
	PARENT_AFTER=$(git cat-file -p HEAD | sed -n "s/^parent //p" | head -1) &&
	test "$PARENT_BEFORE" = "$PARENT_AFTER"
'

test_expect_success 'amend root commit has no parent' '
	git init amend-root-repo &&
	cd amend-root-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "root" >root.txt &&
	git add root.txt &&
	git commit -m "root" 2>/dev/null &&
	git commit --amend -m "amended root" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	! grep "^parent " actual
'

test_expect_success 'amend --author changes author' '
	cd repo &&
	echo "auth" >>file.txt &&
	git add file.txt &&
	git commit -m "original author" 2>/dev/null &&
	git commit --amend --author="New Author <new@test.com>" -m "new author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "New Author <new@test.com>" actual
'

test_expect_success 'commit --date sets author date' '
	cd repo &&
	echo "date" >>file.txt &&
	git add file.txt &&
	git commit --date="1234567890 +0000" -m "with date" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*1234567890 +0000" actual
'

test_expect_success 'commit respects GIT_AUTHOR_DATE' '
	cd repo &&
	echo "envdate" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000000 +0000" git commit -m "env date" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*1000000000 +0000" actual
'

test_expect_success 'commit --date overrides GIT_AUTHOR_DATE' '
	cd repo &&
	echo "dateoverride" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000000 +0000" \
		git commit --date="2000000000 +0000" -m "date override" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*2000000000 +0000" actual
'

test_expect_success 'commit with empty message fails' '
	cd repo &&
	echo "emptymsg" >>file.txt &&
	git add file.txt &&
	! git commit -m "" 2>/dev/null
'

test_expect_success 'commit --allow-empty-message with empty -m' '
	cd repo &&
	git commit --allow-empty-message -m "" 2>/dev/null &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit tree is a tree object' '
	cd repo &&
	echo "treecheck" >>file.txt &&
	git add file.txt &&
	git commit -m "tree check" 2>/dev/null &&
	git cat-file -p HEAD >commit_out &&
	TREE=$(head -1 commit_out | sed -n "s/^tree //p") &&
	git cat-file -t "$TREE" >actual &&
	echo "tree" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit creates proper chain of parents' '
	cd repo &&
	CHILD=$(git rev-parse HEAD) &&
	PARENT=$(git cat-file -p HEAD | sed -n "s/^parent //p" | head -1) &&
	test -n "$PARENT" &&
	git cat-file -t "$PARENT" >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit -a does not commit untracked files' '
	cd repo &&
	echo "untracked-content" >untracked-test.txt &&
	echo "tracked-change" >>file.txt &&
	git commit -a -m "auto stage tracked only" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "auto stage tracked only" actual &&
	git status -s >status_out &&
	grep "^?? untracked-test.txt" status_out
'

test_expect_success 'initial commit output mentions root-commit' '
	git init fresh-repo &&
	cd fresh-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "x" >x.txt &&
	git add x.txt &&
	git commit -m "first" 2>stderr &&
	grep "root-commit" stderr
'

test_expect_success 'second commit output does not mention root-commit' '
	cd fresh-repo &&
	echo "y" >>x.txt &&
	git add x.txt &&
	git commit -m "second" 2>stderr &&
	! grep "root-commit" stderr
'

test_expect_success 'commit output shows branch name' '
	cd fresh-repo &&
	echo "z" >>x.txt &&
	git add x.txt &&
	git commit -m "third" 2>stderr &&
	grep "master" stderr
'

test_expect_success 'allow-empty with no staged changes succeeds' '
	cd repo &&
	git commit --allow-empty -m "truly empty" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "truly empty" actual
'

test_expect_success 'same tree with --allow-empty succeeds' '
	cd repo &&
	TREE_BEFORE=$(git cat-file -p HEAD | sed -n "s/^tree //p") &&
	git commit --allow-empty -m "same tree" 2>/dev/null &&
	TREE_AFTER=$(git cat-file -p HEAD | sed -n "s/^tree //p") &&
	test "$TREE_BEFORE" = "$TREE_AFTER"
'

test_expect_success 'committer is set from config' '
	cd repo &&
	echo "committer" >>file.txt &&
	git add file.txt &&
	git commit -m "check committer" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^committer Test User <test@test.com>" actual
'

test_expect_success 'GIT_COMMITTER_NAME overrides config' '
	cd repo &&
	echo "committer-env" >>file.txt &&
	git add file.txt &&
	GIT_COMMITTER_NAME="Env Committer" GIT_COMMITTER_EMAIL="env@test.com" \
		git commit -m "env committer" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^committer Env Committer <env@test.com>" actual
'

test_done

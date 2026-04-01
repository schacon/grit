#!/bin/sh
# Ported from git/t/t7500-commit-template-squash-signoff.sh
# Tests for commit -F, -m, --allow-empty-message, and related features.
# (Editor-dependent and fixup/squash/template tests are not ported since grit
#  does not implement those features.)

test_description='grit commit -F, -m, and message handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	git init repo &&
	cd repo &&
	git config user.name "C O Mitter" &&
	git config user.email "committer@example.com"
'

test_expect_success 'a basic commit in an empty tree should succeed' '
	cd repo &&
	echo content >foo &&
	git add foo &&
	git commit -m "initial commit" 2>/dev/null
'

test_expect_success 'commit message from file in subdirectory (1)' '
	cd repo &&
	mkdir -p subdir &&
	echo "Log in top directory" >log &&
	echo "Log in sub directory" >subdir/log &&
	cd subdir &&
	git commit --allow-empty -F log 2>/dev/null &&
	cd .. &&
	git cat-file commit HEAD >actual &&
	grep "Log in sub directory" actual
'

test_expect_success 'commit message from file in subdirectory (2)' '
	cd repo &&
	rm -f log &&
	echo "Log in sub directory again" >subdir/log &&
	cd subdir &&
	git commit --allow-empty -F log 2>/dev/null &&
	cd .. &&
	git cat-file commit HEAD >actual &&
	grep "Log in sub directory again" actual
'

test_expect_success 'commit message from stdin' '
	cd repo &&
	echo "Log with foo word" | git commit --allow-empty -F - 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "Log with foo word" actual
'

test_expect_success 'commit -m with simple message' '
	cd repo &&
	echo "new stuff" >>foo &&
	git add foo &&
	git commit -m "simple message" 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "simple message" actual
'

test_expect_success 'commit -F reads message from file' '
	cd repo &&
	echo "file message" >msgfile &&
	echo "fc" >>foo &&
	git add foo &&
	git commit -F msgfile 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "file message" actual
'

test_expect_success 'commit -F - reads from stdin' '
	cd repo &&
	echo "from stdin" | git commit --allow-empty -F - 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "from stdin" actual
'

test_expect_success 'Commit with empty message fails without --allow-empty-message' '
	cd repo &&
	echo "more content" >>foo &&
	git add foo &&
	test_must_fail git commit -m "" 2>/dev/null
'

test_expect_success 'Commit with --allow-empty-message and empty -m succeeds' '
	cd repo &&
	echo "even more" >>foo &&
	git add foo &&
	git commit --allow-empty-message -m "" 2>/dev/null &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_expect_success 'Commit a non-empty message with --allow-empty-message' '
	cd repo &&
	echo "yet more" >>foo &&
	git add foo &&
	git commit --allow-empty-message -m "hello there" 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "hello there" actual
'

test_expect_success 'multiple -m produces multi-paragraph message' '
	cd repo &&
	echo "multi" >>foo &&
	git add foo &&
	git commit -m "one" -m "two" -m "three" 2>/dev/null &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^$/d" commit >actual &&
	{
		echo one &&
		echo &&
		echo two &&
		echo &&
		echo three
	} >expected &&
	test_cmp expected actual
'

test_expect_success 'commit -F with multi-line file' '
	cd repo &&
	printf "line 1\n\nline 3\n" >multiline-msg &&
	echo "multiline" >>foo &&
	git add foo &&
	git commit -F multiline-msg 2>/dev/null &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^$/d" commit >actual &&
	printf "line 1\n\nline 3\n" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit --allow-empty with no changes' '
	cd repo &&
	git commit --allow-empty -m "empty commit" 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "empty commit" actual
'

test_expect_success 'commit --allow-empty keeps same tree' '
	cd repo &&
	TREE_BEFORE=$(git cat-file -p HEAD | sed -n "s/^tree //p") &&
	git commit --allow-empty -m "still empty" 2>/dev/null &&
	TREE_AFTER=$(git cat-file -p HEAD | sed -n "s/^tree //p") &&
	test "$TREE_BEFORE" = "$TREE_AFTER"
'

test_expect_success 'commit -a stages and commits modified tracked files' '
	cd repo &&
	echo "auto-staged content" >>foo &&
	git commit -a -m "auto staged" 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "auto staged" actual
'

test_expect_success 'commit -a does not commit untracked files' '
	cd repo &&
	echo "untracked" >bar &&
	echo "tracked change" >>foo &&
	git commit -a -m "tracked only" 2>/dev/null &&
	git status -s >status_out &&
	grep "^?? bar" status_out
'

test_expect_success 'commit --author overrides identity' '
	cd repo &&
	echo "author" >>foo &&
	git add foo &&
	git commit --author="Custom Author <custom@example.com>" -m "custom author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "author Custom Author <custom@example.com>" actual
'

test_expect_success 'commit --date overrides author date' '
	cd repo &&
	echo "date" >>foo &&
	git add foo &&
	git commit --date="1234567890 +0000" -m "custom date" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*1234567890 +0000" actual
'

test_expect_success 'GIT_AUTHOR_DATE overrides config date' '
	cd repo &&
	echo "adate" >>foo &&
	git add foo &&
	GIT_AUTHOR_DATE="1111111111 +0000" git commit -m "env date" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*1111111111 +0000" actual
'

test_expect_success 'GIT_COMMITTER_DATE overrides committer date' '
	cd repo &&
	echo "cdate" >>foo &&
	git add foo &&
	GIT_COMMITTER_DATE="1222222222 +0000" git commit -m "cdate env" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^committer.*1222222222 +0000" actual
'

test_expect_success 'commit --amend changes message' '
	cd repo &&
	echo "amend me" >>foo &&
	git add foo &&
	git commit -m "before amend" 2>/dev/null &&
	git commit --amend -m "after amend" 2>/dev/null &&
	git cat-file commit HEAD >actual &&
	grep "after amend" actual &&
	! grep "before amend" actual
'

test_expect_success 'commit --amend preserves parent' '
	cd repo &&
	PARENT_BEFORE=$(git cat-file -p HEAD | sed -n "s/^parent //p" | head -1) &&
	git commit --amend -m "amend again" 2>/dev/null &&
	PARENT_AFTER=$(git cat-file -p HEAD | sed -n "s/^parent //p" | head -1) &&
	test "$PARENT_BEFORE" = "$PARENT_AFTER"
'

test_expect_success 'commit --amend --author changes author' '
	cd repo &&
	echo "new-auth" >>foo &&
	git add foo &&
	git commit -m "orig author" 2>/dev/null &&
	git commit --amend --author="New Author <new@example.com>" -m "new author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "New Author <new@example.com>" actual
'

test_expect_success 'commit --amend --date changes date' '
	cd repo &&
	git commit --amend --date="1300000000 +0000" -m "new date" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^author.*1300000000 +0000" actual
'

test_expect_success 'commit --amend --allow-empty-message with empty message' '
	cd repo &&
	echo "aem" >>foo &&
	git add foo &&
	git commit -m "will empty" 2>/dev/null &&
	git commit --amend --allow-empty-message -m "" 2>/dev/null &&
	git cat-file commit HEAD >commit &&
	sed -e "1,/^$/d" commit >body &&
	! grep -q "[^ ]" body
'

test_expect_success 'nothing to commit fails' '
	cd repo &&
	git reset --hard HEAD 2>/dev/null &&
	test_must_fail git commit -m "nothing" 2>/dev/null
'

test_expect_success 'commit -q suppresses output' '
	cd repo &&
	echo "quiet" >>foo &&
	git add foo &&
	git commit -q -m "quiet" 2>stderr &&
	test ! -s stderr
'

test_expect_success 'root commit output mentions root-commit' '
	git init fresh-repo &&
	cd fresh-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo x >x.txt &&
	git add x.txt &&
	git commit -m "first" 2>stderr &&
	grep "root-commit" stderr
'

test_expect_success 'second commit does not mention root-commit' '
	cd fresh-repo &&
	echo y >>x.txt &&
	git add x.txt &&
	git commit -m "second" 2>stderr &&
	! grep "root-commit" stderr
'

test_expect_success 'commit shows branch name in output' '
	cd fresh-repo &&
	echo z >>x.txt &&
	git add x.txt &&
	git commit -m "third" 2>stderr &&
	grep "master" stderr
'

test_expect_success 'GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL override config' '
	cd repo &&
	echo "envauth" >>foo &&
	git add foo &&
	GIT_AUTHOR_NAME="Env Author" GIT_AUTHOR_EMAIL="env@author.com" \
		git commit -m "env author" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "Env Author <env@author.com>" actual
'

test_expect_success 'GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL override config' '
	cd repo &&
	echo "envcmtr" >>foo &&
	git add foo &&
	GIT_COMMITTER_NAME="Env Committer" GIT_COMMITTER_EMAIL="env@committer.com" \
		git commit -m "env committer" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "^committer Env Committer <env@committer.com>" actual
'

test_expect_success 'commit on detached HEAD works' '
	cd repo &&
	git checkout HEAD^0 2>/dev/null &&
	echo "detached" >>foo &&
	git add foo &&
	git commit -m "detached commit" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	grep "detached commit" actual &&
	git checkout master 2>/dev/null
'

test_expect_success 'amend root commit keeps no parent' '
	git init root-amend-repo &&
	cd root-amend-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo root >root.txt &&
	git add root.txt &&
	git commit -m "root" 2>/dev/null &&
	git commit --amend -m "amended root" 2>/dev/null &&
	git cat-file -p HEAD >actual &&
	! grep "^parent " actual
'

test_expect_success 'commit tree is a valid tree object' '
	cd repo &&
	echo "tree-check" >>foo &&
	git add foo &&
	git commit -m "verify tree" 2>/dev/null &&
	TREE=$(git cat-file -p HEAD | head -1 | sed -n "s/^tree //p") &&
	git cat-file -t "$TREE" >actual &&
	echo "tree" >expected &&
	test_cmp expected actual
'

test_expect_success 'commit -F from /dev/null with --allow-empty-message' '
	cd repo &&
	echo "devnull" >>foo &&
	git add foo &&
	git commit --allow-empty-message -F /dev/null 2>/dev/null &&
	git cat-file -t HEAD >actual &&
	echo "commit" >expected &&
	test_cmp expected actual
'

test_done

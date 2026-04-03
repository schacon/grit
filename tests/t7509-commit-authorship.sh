#!/bin/sh
#
# t7509-commit-authorship.sh — GIT_AUTHOR_* env overrides, --author, --date flags
#

test_description='commit authorship overrides'
. ./test-lib.sh

# ── setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: initial repo with a commit' '
	git init authorship-repo &&
	cd authorship-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "base content" >file.txt &&
	git add file.txt &&
	git commit -m "initial commit"
'

# ── GIT_AUTHOR_NAME / GIT_AUTHOR_EMAIL via env ──────────────────────────────

test_expect_success 'GIT_AUTHOR_NAME and GIT_AUTHOR_EMAIL override author' '
	cd authorship-repo &&
	echo "change1" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Custom Author" \
	GIT_AUTHOR_EMAIL="custom@example.com" \
	git commit -m "custom author via env" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Custom Author <custom@example.com>" commit-obj
'

test_expect_success 'GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL override committer' '
	cd authorship-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "change2" >file.txt &&
	git add file.txt &&
	GIT_COMMITTER_NAME="Custom Committer" \
	GIT_COMMITTER_EMAIL="committer@custom.org" \
	git commit -m "custom committer via env" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^committer Custom Committer <committer@custom.org>" commit-obj
'

test_expect_success 'author and committer can differ' '
	cd authorship-repo &&
	echo "change3" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Alice Author" \
	GIT_AUTHOR_EMAIL="alice@example.com" \
	GIT_COMMITTER_NAME="Bob Committer" \
	GIT_COMMITTER_EMAIL="bob@example.com" \
	git commit -m "different author and committer" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Alice Author <alice@example.com>" commit-obj &&
	grep "^committer Bob Committer <bob@example.com>" commit-obj
'

# ── GIT_AUTHOR_DATE via env ─────────────────────────────────────────────────

test_expect_success 'GIT_AUTHOR_DATE overrides author date' '
	cd authorship-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "change4" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="2020-06-15T12:00:00+0000" \
	git commit -m "custom author date" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author.*2020" commit-obj
'

# ── --author flag ────────────────────────────────────────────────────────────

test_expect_success '--author flag overrides author identity' '
	cd authorship-repo &&
	echo "change5" >file.txt &&
	git add file.txt &&
	git commit --author="Flag Author <flag@example.com>" -m "author via flag" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Flag Author <flag@example.com>" commit-obj
'

test_expect_success '--author flag does not affect committer' '
	cd authorship-repo &&
	echo "change6" >file.txt &&
	git add file.txt &&
	GIT_COMMITTER_NAME="Still Committer" \
	GIT_COMMITTER_EMAIL="still@committer.com" \
	git commit --author="Only Author <only@author.com>" -m "author only" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Only Author <only@author.com>" commit-obj &&
	grep "^committer Still Committer <still@committer.com>" commit-obj
'

# ── --date flag ──────────────────────────────────────────────────────────────

test_expect_success '--date flag overrides author date' '
	cd authorship-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "change7" >file.txt &&
	git add file.txt &&
	git commit --date="2019-03-14T00:00:00+0000" -m "custom date via flag" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author.*2019" commit-obj
'

# ── log --format verifies authorship fields ──────────────────────────────────

test_expect_success 'log --format=%an shows author name' '
	cd authorship-repo &&
	echo "change8" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Logged Author" \
	GIT_AUTHOR_EMAIL="logged@example.com" \
	git commit -m "for log check" &&
	git log --format="%an" -n 1 >actual &&
	echo "Logged Author" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%ae shows author email' '
	cd authorship-repo &&
	git log --format="%ae" -n 1 >actual &&
	echo "logged@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'log --format=%cn/%ce shows committer' '
	cd authorship-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "change9" >file.txt &&
	git add file.txt &&
	GIT_COMMITTER_NAME="Logged Committer" \
	GIT_COMMITTER_EMAIL="logcommit@example.com" \
	git commit -m "committer log check" &&
	git log --format="%cn <%ce>" -n 1 >actual &&
	echo "Logged Committer <logcommit@example.com>" >expect &&
	test_cmp expect actual
'

# ── combined: --author + GIT_COMMITTER_* ─────────────────────────────────────

test_expect_success '--author + GIT_COMMITTER_* combined' '
	cd authorship-repo &&
	echo "change10" >file.txt &&
	git add file.txt &&
	GIT_COMMITTER_NAME="C Name" \
	GIT_COMMITTER_EMAIL="c@e.com" \
	git commit \
		--author="A Name <a@e.com>" \
		-m "author and committer overrides" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author A Name <a@e.com>" commit-obj &&
	grep "^committer C Name <c@e.com>" commit-obj
'

# ── GIT_COMMITTER_DATE via env ───────────────────────────────────────────────

test_expect_success 'GIT_COMMITTER_DATE overrides committer date' '
	cd authorship-repo &&
	echo "change11" >file.txt &&
	git add file.txt &&
	GIT_COMMITTER_DATE="2021-01-01T00:00:00+0000" \
	git commit -m "custom committer date" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^committer.*2021" commit-obj
'

# ── author name with special characters ──────────────────────────────────────

test_expect_success 'author name with accented characters' '
	cd authorship-repo &&
	echo "change12" >file.txt &&
	git add file.txt &&
	git commit --author="José García <jose@example.com>" -m "accented author" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author José García" commit-obj
'

test_expect_success 'author email with plus addressing' '
	cd authorship-repo &&
	echo "change13" >file.txt &&
	git add file.txt &&
	git commit --author="User <user+tag@example.com>" -m "plus email" &&
	git cat-file -p HEAD >commit-obj &&
	grep "user+tag@example.com" commit-obj
'

# ── amend preserves authorship ───────────────────────────────────────────────

test_expect_success 'amend changes message but keeps tree' '
	cd authorship-repo &&
	echo "change14" >file.txt &&
	git add file.txt &&
	git commit -m "original" &&
	git cat-file -p HEAD | head -1 | awk "{print \$2}" >orig_tree &&
	git commit --amend -m "amended" &&
	git cat-file -p HEAD | head -1 | awk "{print \$2}" >new_tree &&
	test_cmp orig_tree new_tree
'

test_expect_success 'amend with --author changes author' '
	cd authorship-repo &&
	git commit --amend --author="New Auth <new@test.com>" -m "new author amend" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author New Auth <new@test.com>" commit-obj
'

# ── log format for dates ─────────────────────────────────────────────────────

test_expect_success 'log --format=%ai shows author date' '
	cd authorship-repo &&
	echo "change15" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="2022-07-04T12:00:00+0000" \
	git commit -m "date format check" &&
	git log --format="%ai" -n 1 >actual &&
	grep "2022" actual
'

test_expect_success 'log --format=%cd shows committer date info' '
	cd authorship-repo &&
	git log --format="%cd" -n 1 >actual &&
	# Should have some date output
	test -s actual
'

# ── config user overrides ────────────────────────────────────────────────────

test_expect_success 'config user.name/email used as defaults' '
	cd authorship-repo &&
	git config user.name "Config Author" &&
	git config user.email "config@example.com" &&
	sane_unset GIT_AUTHOR_NAME &&
	sane_unset GIT_AUTHOR_EMAIL &&
	echo "change16" >file.txt &&
	git add file.txt &&
	git commit -m "from config" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Config Author <config@example.com>" commit-obj
'

test_expect_success 'env vars override config user' '
	cd authorship-repo &&
	echo "change17" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_NAME="Env Author" \
	GIT_AUTHOR_EMAIL="env@example.com" \
	git commit -m "env overrides config" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author Env Author <env@example.com>" commit-obj
'

# ── --date with various formats ──────────────────────────────────────────────

test_expect_success '--date with epoch timestamp' '
	cd authorship-repo &&
	echo "change18" >file.txt &&
	git add file.txt &&
	git commit --date="1000000000 +0000" -m "epoch date" &&
	git cat-file -p HEAD >commit-obj &&
	grep "^author.*1000000000" commit-obj
'

test_done

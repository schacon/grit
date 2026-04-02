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

test_done

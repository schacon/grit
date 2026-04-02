#!/bin/sh
# Ported from git/t/t1100-commit-tree-options.sh

test_description='grit commit-tree options test

This test checks that grit commit-tree can create a specific commit
object by defining all environment variables that it understands.

Also make sure that command line parser understands the normal
"flags first and then non flag arguments" command line.
'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success \
  'test preparation: init and write empty tree' \
  'git init repo &&
     cd repo &&
     git write-tree >treeid'

test_expect_success \
  'construct commit' \
  'cd repo &&
     echo comment text |
     GIT_AUTHOR_NAME="Author Name" \
     GIT_AUTHOR_EMAIL="author@email" \
     GIT_AUTHOR_DATE="2005-05-26 23:00" \
     GIT_COMMITTER_NAME="Committer Name" \
     GIT_COMMITTER_EMAIL="committer@email" \
     GIT_COMMITTER_DATE="2005-05-26 23:30" \
     TZ=GMT git commit-tree $(cat treeid) >commitid 2>/dev/null'

test_expect_success \
  'read commit' \
  'cd repo &&
     git cat-file commit $(cat commitid) >commit'

test_expect_success \
  'compare commit' \
  'cd repo &&
     tree=$(cat treeid) &&
     {
     	echo "tree $tree" &&
     	echo "author Author Name <author@email> 1117148400 +0000" &&
     	echo "committer Committer Name <committer@email> 1117150200 +0000" &&
     	echo &&
     	echo "comment text"
     } >expected &&
     git cat-file commit $(cat commitid) >commit &&
     test_cmp expected commit'

test_expect_success \
  'compare commit' \
  'cd repo &&
     tree=$(cat treeid) &&
     {
     	echo "tree $tree" &&
     	echo "author Author Name <author@email> 1117148400 +0000" &&
     	echo "committer Committer Name <committer@email> 1117150200 +0000" &&
     	echo &&
     	echo "comment text"
     } >expected &&
     test_cmp expected commit'

test_expect_success 'flags and then non flags' '
	cd repo &&
	test_tick &&
	echo comment text |
	git commit-tree $(cat treeid) >commitid &&
	echo comment text |
	git commit-tree $(cat treeid) -p $(cat commitid) >childid-1 &&
	echo comment text |
	git commit-tree -p $(cat commitid) $(cat treeid) >childid-2 &&
	test_cmp childid-1 childid-2 &&
	git commit-tree $(cat treeid) -m foo >childid-3 &&
	git commit-tree -m foo $(cat treeid) >childid-4 &&
	test_cmp childid-3 childid-4
'

# ---- Wave 5: additional commit-tree tests ----

test_expect_success 'commit-tree with -m flag' '
	cd repo &&
	test_tick &&
	git commit-tree -m "message via -m" $(cat treeid) >commitid-m &&
	git cat-file commit $(cat commitid-m) >commit-m &&
	grep "message via -m" commit-m
'

test_expect_success 'commit-tree with multiple -p parents' '
	cd repo &&
	test_tick &&
	echo "parent 1" | git commit-tree $(cat treeid) >p1 &&
	echo "parent 2" | git commit-tree $(cat treeid) >p2 &&
	echo "merge" | git commit-tree $(cat treeid) -p $(cat p1) -p $(cat p2) >merge &&
	git cat-file commit $(cat merge) >merge-commit &&
	grep "^parent $(cat p1)" merge-commit &&
	grep "^parent $(cat p2)" merge-commit
'

test_expect_success 'commit-tree object is a valid commit' '
	cd repo &&
	test_tick &&
	echo "check type" | git commit-tree $(cat treeid) >oid &&
	git cat-file -t $(cat oid) >actual &&
	echo commit >expected &&
	test_cmp expected actual
'

test_expect_success 'commit-tree with -F reads message from file' '
	cd repo &&
	test_tick &&
	echo "message from file" >msg-file &&
	git commit-tree -F msg-file $(cat treeid) >commitid-f &&
	git cat-file commit $(cat commitid-f) >commit-f &&
	grep "message from file" commit-f
'

test_expect_success 'commit-tree respects GIT_COMMITTER_NAME/EMAIL' '
	cd repo &&
	test_tick &&
	echo "env test" |
	GIT_COMMITTER_NAME="Custom Committer" \
	GIT_COMMITTER_EMAIL="custom@example.com" \
	git commit-tree $(cat treeid) >commitid-env &&
	git cat-file commit $(cat commitid-env) >commit-env &&
	grep "committer Custom Committer <custom@example.com>" commit-env
'

test_expect_success 'commit-tree respects GIT_AUTHOR_NAME/EMAIL' '
	cd repo &&
	test_tick &&
	echo "author env test" |
	GIT_AUTHOR_NAME="Custom Author" \
	GIT_AUTHOR_EMAIL="author@custom.com" \
	git commit-tree $(cat treeid) >commitid-aenv &&
	git cat-file commit $(cat commitid-aenv) >commit-aenv &&
	grep "author Custom Author <author@custom.com>" commit-aenv
'

test_expect_success 'commit-tree with parent has correct parent field' '
	cd repo &&
	test_tick &&
	echo "child" | git commit-tree $(cat treeid) -p $(cat commitid-m) >childid &&
	git cat-file commit $(cat childid) >child-commit &&
	grep "^parent $(cat commitid-m)" child-commit
'

test_expect_success 'commit-tree root commit has no parent' '
	cd repo &&
	test_tick &&
	echo "root" | git commit-tree $(cat treeid) >rootid &&
	git cat-file commit $(cat rootid) >root-commit &&
	! grep "^parent" root-commit
'

# ---- more commit-tree tests ----

test_expect_success 'commit-tree with empty message via -m' '
	cd repo &&
	test_tick &&
	git commit-tree -m "" $(cat treeid) >emptyid &&
	git cat-file commit $(cat emptyid) >empty-commit &&
	git cat-file -t $(cat emptyid) >type &&
	echo commit >expect-type &&
	test_cmp expect-type type
'

test_expect_success 'commit-tree with multi-line message' '
	cd repo &&
	test_tick &&
	printf "line one\nline two\n" >multi-msg &&
	git commit-tree -F multi-msg $(cat treeid) >multiid &&
	git cat-file commit $(cat multiid) >multi-commit &&
	grep "line one" multi-commit &&
	grep "line two" multi-commit
'

test_expect_success 'commit-tree with three parents' '
	cd repo &&
	test_tick &&
	echo "p1" | git commit-tree $(cat treeid) >pp1 &&
	echo "p2" | git commit-tree $(cat treeid) >pp2 &&
	echo "p3" | git commit-tree $(cat treeid) >pp3 &&
	echo "octopus" | git commit-tree $(cat treeid) \
		-p $(cat pp1) -p $(cat pp2) -p $(cat pp3) >octid &&
	git cat-file commit $(cat octid) >oct-commit &&
	test $(grep -c "^parent" oct-commit) = 3
'

test_done

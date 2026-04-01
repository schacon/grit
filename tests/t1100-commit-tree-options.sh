#!/bin/sh
# Ported from git/t/t1100-commit-tree-options.sh

test_description='gust commit-tree options test

This test checks that gust commit-tree can create a specific commit
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
     cat >expected <<-EOF &&
     tree $tree
     author Author Name <author@email> 1117148400 +0000
     committer Committer Name <committer@email> 1117150200 +0000
     
     comment text
     EOF
     git cat-file $(cat commitid) >commit'

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

test_done

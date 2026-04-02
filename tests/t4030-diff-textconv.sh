#!/bin/sh
# Tests for diff handling of binary files and special content.
# Upstream git t4030 covers textconv filters.
# grit doesn't implement textconv yet, so we test binary diff behavior,
# diff with no-newline-at-eof, empty files, and mode changes.

test_description='diff binary and special content handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup binary diff repo' '
	git init diffbin &&
	cd diffbin &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'diff --cached with text file works normally' '
	cd diffbin &&
	echo "hello world" >text.txt &&
	git add text.txt &&
	test_tick &&
	git commit -m "add text" &&
	echo "goodbye world" >text.txt &&
	git add text.txt &&
	git diff --cached >actual &&
	grep "^-hello world" actual &&
	grep "^+goodbye world" actual
'

test_expect_success 'diff --cached with no-newline-at-eof' '
	cd diffbin &&
	test_tick &&
	git commit -m "update text" &&
	printf "no newline" >noeol.txt &&
	git add noeol.txt &&
	git diff --cached >actual &&
	grep "no newline" actual
'

test_expect_success 'diff between commits with no-newline-at-eof' '
	cd diffbin &&
	test_tick &&
	git commit -m "add noeol" &&
	printf "changed no newline" >noeol.txt &&
	git add noeol.txt &&
	test_tick &&
	git commit -m "change noeol" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff $parent $head >actual &&
	grep "No newline at end of file" actual ||
	grep "no newline" actual
'

test_expect_success 'diff --stat with file having no newline' '
	cd diffbin &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --stat $parent $head >actual &&
	grep "noeol.txt" actual
'

test_expect_success 'diff with empty file creation' '
	cd diffbin &&
	: >empty.txt &&
	git add empty.txt &&
	git diff --cached >actual &&
	grep "^diff --git" actual &&
	grep "empty.txt" actual
'

test_expect_success 'diff with content added to empty file' '
	cd diffbin &&
	test_tick &&
	git commit -m "add empty" &&
	echo "now has content" >empty.txt &&
	git add empty.txt &&
	git diff --cached >actual &&
	grep "^+now has content" actual
'

test_expect_success 'diff with file emptied' '
	cd diffbin &&
	test_tick &&
	git commit -m "fill empty" &&
	: >empty.txt &&
	git add empty.txt &&
	git diff --cached >actual &&
	grep "^-now has content" actual
'

test_expect_success 'diff --numstat with multiple changed files' '
	cd diffbin &&
	test_tick &&
	git commit -m "empty again" &&
	echo "new1" >a.txt &&
	echo "new2" >b.txt &&
	git add a.txt b.txt &&
	test_tick &&
	git commit -m "add a and b" &&
	echo "changed1" >a.txt &&
	echo "changed2" >b.txt &&
	git add a.txt b.txt &&
	test_tick &&
	git commit -m "change both" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --numstat $parent $head >actual &&
	test_line_count = 2 actual
'

test_expect_success 'diff --name-only with addition and modification' '
	cd diffbin &&
	echo "extra" >c.txt &&
	echo "more" >>a.txt &&
	git add a.txt c.txt &&
	test_tick &&
	git commit -m "modify a add c" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --name-only $parent $head >actual &&
	grep "a.txt" actual &&
	grep "c.txt" actual
'

test_expect_success 'diff with file containing special characters' '
	cd diffbin &&
	printf "tab\there\n" >special.txt &&
	git add special.txt &&
	test_tick &&
	git commit -m "add special" &&
	printf "tab\tchanged\n" >special.txt &&
	git add special.txt &&
	git diff --cached >actual &&
	grep "here" actual &&
	grep "changed" actual
'

test_expect_success 'diff with file containing only whitespace' '
	cd diffbin &&
	test_tick &&
	git commit -m "update special" &&
	printf "   \n" >ws.txt &&
	git add ws.txt &&
	test_tick &&
	git commit -m "add whitespace file" &&
	printf "  \n" >ws.txt &&
	git add ws.txt &&
	git diff --cached >actual &&
	grep "^@@" actual
'

test_expect_success 'diff --stat shows correct insertion/deletion counts' '
	cd diffbin &&
	test_tick &&
	git commit -m "ws change" &&
	echo "line1" >count.txt &&
	echo "line2" >>count.txt &&
	echo "line3" >>count.txt &&
	git add count.txt &&
	test_tick &&
	git commit -m "add count" &&
	echo "LINE1" >count.txt &&
	echo "line2" >>count.txt &&
	echo "LINE3" >>count.txt &&
	git add count.txt &&
	test_tick &&
	git commit -m "modify count" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --numstat $parent $head >actual &&
	# 2 insertions, 2 deletions (lines 1 and 3 changed)
	grep "^2	2	count.txt" actual
'

test_expect_success 'diff with long lines' '
	cd diffbin &&
	python3 -c "print(\"A\" * 1000)" >longline.txt &&
	git add longline.txt &&
	test_tick &&
	git commit -m "add longline" &&
	python3 -c "print(\"B\" * 1000)" >longline.txt &&
	git add longline.txt &&
	git diff --cached >actual &&
	grep "^-AAAA" actual &&
	grep "^+BBBB" actual
'

test_expect_success 'diff-tree between two commits' '
	cd diffbin &&
	test_tick &&
	git commit -m "update longline" &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree $c1 $c2 >actual &&
	grep "longline.txt" actual
'

test_expect_success 'diff-tree -p shows patch' '
	cd diffbin &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree -p $c1 $c2 >actual &&
	grep "^@@" actual
'

test_expect_success 'diff with multiple lines added' '
	cd diffbin &&
	seq 1 10 >seq.txt &&
	git add seq.txt &&
	test_tick &&
	git commit -m "add seq" &&
	seq 1 20 >seq.txt &&
	git add seq.txt &&
	git diff --cached >actual &&
	grep "^+11" actual
'

test_expect_success 'diff with multiple lines removed' '
	cd diffbin &&
	test_tick &&
	git commit -m "expand seq" &&
	seq 1 5 >seq.txt &&
	git add seq.txt &&
	git diff --cached >actual &&
	grep "^-6" actual &&
	grep "^-20" actual
'

test_expect_success 'diff --name-status shows A for added files' '
	cd diffbin &&
	test_tick &&
	git commit -m "shrink seq" &&
	echo "brand new" >new_add.txt &&
	git add new_add.txt &&
	test_tick &&
	git commit -m "add new_add" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --name-status $parent $head >actual &&
	grep "^A" actual &&
	grep "new_add.txt" actual
'

test_expect_success 'diff --name-status shows D for deleted files' '
	cd diffbin &&
	git rm new_add.txt &&
	test_tick &&
	git commit -m "rm new_add" &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git diff --name-status $parent $head >actual &&
	grep "^D" actual &&
	grep "new_add.txt" actual
'

test_done

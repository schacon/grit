#!/bin/sh
#
# t0003-attributes.sh — .gitattributes file handling and attribute effects
#
# Note: grit does not yet implement check-attr. These tests verify that
# .gitattributes files are tracked/committed properly and probe attribute
# effects where possible.
#

test_description='.gitattributes handling'
. ./test-lib.sh

# ── setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: init repo' '
	git init attr-repo &&
	cd attr-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

# ── basic .gitattributes tracking ────────────────────────────────────────────

test_expect_success '.gitattributes can be added and committed' '
	cd attr-repo &&
	echo "*.txt text" >.gitattributes &&
	git add .gitattributes &&
	git commit -m "add .gitattributes" &&
	git cat-file -p HEAD >commit-obj &&
	grep "add .gitattributes" commit-obj
'

test_expect_success '.gitattributes content is stored correctly' '
	cd attr-repo &&
	git show HEAD:.gitattributes >actual &&
	echo "*.txt text" >expect &&
	test_cmp expect actual
'

test_expect_success '.gitattributes with multiple patterns' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	*.txt text
	*.bin binary
	*.sh text eol=lf
	*.png -diff
	Makefile text
	EOF
	git add .gitattributes &&
	git commit -m "multi-pattern .gitattributes" &&
	git show HEAD:.gitattributes >actual &&
	grep "\\*.txt text" actual &&
	grep "\\*.bin binary" actual &&
	grep "\\*.sh text eol=lf" actual &&
	grep "\\*.png -diff" actual &&
	grep "Makefile text" actual
'

# ── subdirectory .gitattributes ──────────────────────────────────────────────

test_expect_success 'subdirectory .gitattributes is tracked' '
	cd attr-repo &&
	mkdir -p sub &&
	echo "*.dat binary" >sub/.gitattributes &&
	git add sub/.gitattributes &&
	git commit -m "add sub/.gitattributes" &&
	git show HEAD:sub/.gitattributes >actual &&
	echo "*.dat binary" >expect &&
	test_cmp expect actual
'

test_expect_success 'nested .gitattributes files coexist' '
	cd attr-repo &&
	git ls-files >actual &&
	grep "^.gitattributes$" actual &&
	grep "^sub/.gitattributes$" actual
'

# ── info/attributes ──────────────────────────────────────────────────────────

test_expect_success 'info/attributes directory can be created' '
	cd attr-repo &&
	mkdir -p .git/info &&
	echo "*.log -diff" >.git/info/attributes &&
	test -f .git/info/attributes
'

test_expect_success 'info/attributes is not tracked by git' '
	cd attr-repo &&
	git status >status-out 2>&1 &&
	! grep "info/attributes" status-out
'

# ── .gitattributes with comment and blank lines ─────────────────────────────

test_expect_success '.gitattributes handles comments and blank lines' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	# This is a comment
	*.txt text

	# Binary files
	*.bin binary
	EOF
	git add .gitattributes &&
	git commit -m "attrs with comments" &&
	git show HEAD:.gitattributes >actual &&
	grep "^# This is a comment" actual &&
	grep "\\*.txt text" actual &&
	grep "\\*.bin binary" actual
'

# ── .gitattributes with various attribute forms ──────────────────────────────

test_expect_success '.gitattributes stores set/unset/value/unspecified forms' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	*.c	text diff
	*.o	-text -diff
	*.pdf	binary
	*.html	text=auto diff=html
	*.jpg	-text -diff -merge
	EOF
	git add .gitattributes &&
	git commit -m "various attribute forms" &&
	git show HEAD:.gitattributes >actual &&
	grep "\\*.c" actual &&
	grep "\\*.o" actual &&
	grep "\\*.pdf" actual &&
	grep "\\*.html" actual &&
	grep "\\*.jpg" actual
'

# ── check-attr is not supported ──────────────────────────────────────────────

test_expect_success 'check-attr command is not yet implemented' '
	cd attr-repo &&
	test_must_fail git check-attr text file.txt 2>err &&
	grep -i "unrecognized\|unknown\|not.*found" err
'

# ── .gitattributes survives across branches ──────────────────────────────────

test_expect_success '.gitattributes content differs across branches' '
	cd attr-repo &&
	git checkout -b feature-attrs &&
	echo "*.rs text diff" >.gitattributes &&
	git add .gitattributes &&
	git commit -m "rust attrs on feature branch" &&
	git show HEAD:.gitattributes >feature-attrs &&
	grep "\\*.rs text diff" feature-attrs &&
	git checkout master &&
	git show HEAD:.gitattributes >master-attrs &&
	! grep "\\*.rs" master-attrs
'

# ── .gitattributes with path patterns ────────────────────────────────────────

test_expect_success '.gitattributes with directory and negation patterns stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	docs/**/*.md text
	!important.bin
	/root-only.txt text
	EOF
	git add .gitattributes &&
	git commit -m "path pattern attrs" &&
	git show HEAD:.gitattributes >actual &&
	grep "docs/\\*\\*/\\*.md" actual &&
	grep "!important.bin" actual &&
	grep "/root-only.txt" actual
'

# ── large .gitattributes ────────────────────────────────────────────────────

test_expect_success 'large .gitattributes file is handled' '
	cd attr-repo &&
	i=0 &&
	while test $i -lt 100
	do
		echo "file${i}.txt text" >>.gitattributes-big
		i=$(($i + 1))
	done &&
	cp .gitattributes-big .gitattributes &&
	git add .gitattributes &&
	git commit -m "large gitattributes" &&
	git show HEAD:.gitattributes >actual &&
	test_line_count = 100 actual
'

test_done

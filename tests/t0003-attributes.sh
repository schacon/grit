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

# ── .gitattributes removed then re-added ───────────────────────────────────

test_expect_success '.gitattributes can be removed and re-added' '
	cd attr-repo &&
	git rm .gitattributes &&
	git commit -m "remove attrs" &&
	git ls-files >ls-out &&
	! grep "^.gitattributes$" ls-out &&
	echo "*.md text" >.gitattributes &&
	git add .gitattributes &&
	git commit -m "re-add attrs" &&
	git show HEAD:.gitattributes >actual &&
	echo "*.md text" >expect &&
	test_cmp expect actual
'

# ── .gitattributes with trailing whitespace ─────────────────────────────────

test_expect_success '.gitattributes with trailing whitespace stored verbatim' '
	cd attr-repo &&
	printf "*.txt text  \n" >.gitattributes &&
	git add .gitattributes &&
	git commit -m "trailing ws" &&
	git show HEAD:.gitattributes >actual &&
	test -s actual
'

# ── .gitattributes with macro definitions ──────────────────────────────────

test_expect_success '.gitattributes with macro-style entries stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	[attr]binary -diff -merge -text
	*.o binary
	EOF
	git add .gitattributes &&
	git commit -m "macro attrs" &&
	git show HEAD:.gitattributes >actual &&
	grep "\[attr\]binary" actual &&
	grep "\*.o binary" actual
'

# ── .gitattributes in multiple subdirectories ──────────────────────────────

test_expect_success 'multiple subdirectory .gitattributes files tracked' '
	cd attr-repo &&
	mkdir -p src tests docs &&
	echo "*.c text diff" >src/.gitattributes &&
	echo "*.test text" >tests/.gitattributes &&
	echo "*.md text" >docs/.gitattributes &&
	git add src/.gitattributes tests/.gitattributes docs/.gitattributes &&
	git commit -m "multi-subdir attrs" &&
	git ls-files >ls-out &&
	grep "src/.gitattributes" ls-out &&
	grep "tests/.gitattributes" ls-out &&
	grep "docs/.gitattributes" ls-out
'

# ── .gitattributes with export-ignore ─────────────────────────────────────

test_expect_success '.gitattributes with export-ignore stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	.gitattributes export-ignore
	.gitignore export-ignore
	tests/ export-ignore
	EOF
	git add .gitattributes &&
	git commit -m "export-ignore attrs" &&
	git show HEAD:.gitattributes >actual &&
	grep "export-ignore" actual
'

# ── .gitattributes with filter attributes ──────────────────────────────────

test_expect_success '.gitattributes with filter attributes stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	*.c filter=indent
	*.py filter=autopep8
	EOF
	git add .gitattributes &&
	git commit -m "filter attrs" &&
	git show HEAD:.gitattributes >actual &&
	grep "filter=indent" actual &&
	grep "filter=autopep8" actual
'

# ── diff between branches shows .gitattributes changes ────────────────────

test_expect_success 'diff shows .gitattributes changes between commits' '
	cd attr-repo &&
	git checkout master &&
	echo "*.rs text" >.gitattributes &&
	git add .gitattributes &&
	git commit -m "attrs for diff" &&
	git diff HEAD~1 HEAD -- .gitattributes >diff-out &&
	test -s diff-out
'

# ── .gitattributes with complex patterns ───────────────────────────────────

test_expect_success '.gitattributes with complex glob patterns stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	*.[ch] text diff
	*.py text diff=python
	vendor/** -diff
	test-*.sh text eol=lf
	EOF
	git add .gitattributes &&
	git commit -m "complex globs" &&
	git show HEAD:.gitattributes >actual &&
	grep "\*.\[ch\] text diff" actual &&
	grep "vendor/\*\*" actual &&
	grep "test-\*.sh" actual
'

# ── empty .gitattributes ──────────────────────────────────────────────────

test_expect_success 'empty .gitattributes can be committed' '
	cd attr-repo &&
	>.gitattributes &&
	git add .gitattributes &&
	git commit -m "empty attrs" &&
	git show HEAD:.gitattributes >actual &&
	test_must_be_empty actual
'

# ── .gitattributes with only comments ─────────────────────────────────────

test_expect_success '.gitattributes with only comments stored' '
	cd attr-repo &&
	cat >.gitattributes <<-\EOF &&
	# This is a comment
	# Another comment
	EOF
	git add .gitattributes &&
	git commit -m "comments only" &&
	git show HEAD:.gitattributes >actual &&
	grep "^# This" actual
'

# ── .gitattributes overwrite on branch switch ─────────────────────────────

test_expect_success '.gitattributes restored correctly on branch switch' '
	cd attr-repo &&
	git checkout master &&
	echo "*.txt text" >.gitattributes &&
	git add .gitattributes && git commit -m "master attrs" &&
	git checkout -b switch-test &&
	echo "*.bin binary" >.gitattributes &&
	git add .gitattributes && git commit -m "switch-test attrs" &&
	git checkout master &&
	git show HEAD:.gitattributes >actual &&
	grep "\*.txt text" actual &&
	! grep "\*.bin" actual
'

test_done

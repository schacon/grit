#!/bin/sh

test_description='handling of deep trees in various commands'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'create moderately deep tree via fast-import' '
	{
		echo "commit refs/tags/deep50" &&
		echo "committer foo <foo@example.com> 1234 -0000" &&
		echo "data <<EOF" &&
		echo "deep tree commit" &&
		echo "EOF" &&
		printf "M 100644 inline " &&
		i=0 &&
		while test $i -lt 50
		do
			printf "a/"
			i=$((i+1))
		done &&
		echo "file" &&
		echo "data <<EOF" &&
		echo "the file contents" &&
		echo "EOF" &&
		echo
	} | git fast-import
'

test_expect_success 'ls-tree -r can read deep tree' '
	oid=$(git rev-parse deep50) &&
	git ls-tree -r "$oid" >actual &&
	test_line_count = 1 actual &&
	grep "file$" actual
'

test_expect_success 'rev-list lists commit for deep tree' '
	oid=$(git rev-parse deep50) &&
	git rev-list "$oid" >actual &&
	test_line_count = 1 actual
'

test_expect_success 'create shallow tree' '
	{
		echo "commit refs/tags/shallow" &&
		echo "committer foo <foo@example.com> 1234 -0000" &&
		echo "data <<EOF" &&
		echo "shallow tree" &&
		echo "EOF" &&
		echo "M 100644 inline file1" &&
		echo "data <<EOF" &&
		echo "content1" &&
		echo "EOF" &&
		echo "M 100644 inline file2" &&
		echo "data <<EOF" &&
		echo "content2" &&
		echo "EOF" &&
		echo
	} | git fast-import
'

test_expect_success 'ls-tree on shallow tree shows two files' '
	oid=$(git rev-parse shallow) &&
	git ls-tree "$oid" >actual &&
	test_line_count = 2 actual
'

test_expect_success 'diff-tree between deep and shallow works' '
	deep=$(git rev-parse deep50) &&
	shallow=$(git rev-parse shallow) &&
	git diff-tree "$deep" "$shallow" >actual &&
	test -s actual
'

test_done

#!/bin/sh
test_description='grit diff binary file handling'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	printf "hello\nworld\n" >text.txt &&
	printf "\x00\x01\x02\x03binary-content" >bin.dat &&
	git add text.txt bin.dat &&
	git commit -m "initial"
'

test_expect_success 'diff detects binary file change' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	git diff >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --stat shows binary file' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	git diff --stat >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --name-only lists binary file' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	git diff --name-only >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --name-status shows M for modified binary' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	git diff --name-status >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --numstat shows binary file' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	git diff --numstat >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --exit-code detects binary change' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff --quiet detects binary change' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	test_must_fail git diff --quiet
'

test_expect_success 'diff shows both text and binary changes' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	echo "modified" >text.txt &&
	git diff --name-only >out &&
	grep "bin\.dat" out &&
	grep "text\.txt" out
'

test_expect_success 'diff between commits with binary change' '
	cd repo &&
	printf "\x00\x01\x02\x04changed-binary" >bin.dat &&
	echo "modified" >text.txt &&
	git add . &&
	git commit -m "modify binary" &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree -r "$c1" "$c2" >out &&
	grep "bin\.dat" out &&
	grep "text\.txt" out
'

test_expect_success 'diff-tree --name-status for binary change' '
	cd repo &&
	c1=$(git rev-parse HEAD~1) &&
	c2=$(git rev-parse HEAD) &&
	git diff-tree -r --name-status "$c1" "$c2" >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff for newly added binary file' '
	cd repo &&
	printf "\xff\xfe\xfd" >new-bin.dat &&
	git add new-bin.dat &&
	git diff --cached >out &&
	grep "new-bin\.dat" out
'

test_expect_success 'diff for deleted binary file' '
	cd repo &&
	git commit -m "add new-bin" &&
	git rm new-bin.dat &&
	git diff --cached >out &&
	grep "new-bin\.dat" out
'

test_done

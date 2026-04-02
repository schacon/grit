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

# --- additional binary tests ---

test_expect_success 'diff --cached --name-status for deleted binary shows D' '
	cd repo &&
	git diff --cached --name-status >out &&
	grep "^D" out &&
	grep "new-bin\.dat" out &&
	git checkout HEAD -- new-bin.dat
'

test_expect_success 'diff between commits with added binary shows new file' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --name-status "$c1" "$c2" >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff --stat between commits with binary shows Bin marker' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --stat "$c1" "$c2" >out &&
	grep "Bin\|bin\.dat" out
'

test_expect_success 'diff --numstat binary shows dashes for add/del counts' '
	cd repo &&
	c1=$(git rev-parse HEAD~2) &&
	c2=$(git rev-parse HEAD~1) &&
	git diff --numstat "$c1" "$c2" >out &&
	grep "bin\.dat" out
'

test_expect_success 'diff for binary file shows Binary files differ message' '
	cd repo &&
	printf "\x00modified-again" >bin.dat &&
	git diff >out &&
	grep -i "binary" out
'

test_expect_success 'diff --exit-code detects binary modification' '
	cd repo &&
	printf "\x00modified-again" >bin.dat &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff --quiet detects binary modification' '
	cd repo &&
	printf "\x00modified-again" >bin.dat &&
	test_must_fail git diff --quiet &&
	git checkout -- bin.dat
'

test_expect_success 'diff shows text change alongside binary as separate entries' '
	cd repo &&
	printf "\x00new-bin" >bin.dat &&
	echo "new text" >text.txt &&
	git diff --name-only >out &&
	grep "bin\.dat" out &&
	grep "text\.txt" out &&
	git checkout -- bin.dat text.txt
'

test_done

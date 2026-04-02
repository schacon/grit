#!/bin/sh
test_description='grit diff context line options (-U / --unified)

Tests the -U / --unified option for controlling context lines in
unified diff output. Note: grit currently has limitations in its
unified diff output (context/added lines may be incomplete).'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq 1 20); do echo "line$i"; done >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'diff with default context produces output' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff >out &&
	grep "file\.txt" out &&
	grep "line10" out
'

test_expect_success '-U0 produces output' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U0 >out &&
	grep "file\.txt" out &&
	grep "line10" out
'

test_expect_success '-U1 produces output' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U1 >out &&
	grep "file\.txt" out
'

test_expect_success '-U5 produces output' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U5 >out &&
	grep "file\.txt" out
'

test_expect_success '--unified=0 accepted as -U0' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff --unified=0 >out &&
	grep "file\.txt" out
'

test_expect_success '--unified=2 accepted as -U2' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff --unified=2 >out &&
	grep "file\.txt" out
'

test_expect_success '-U0 hunk header present' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U0 >out &&
	grep "^@@" out
'

test_expect_success '-U100 (large context) produces output' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U100 >out &&
	grep "file\.txt" out
'

test_expect_success 'context with multiple changed lines' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "3" || test "$i" = "17"; then
			echo "CHANGED$i"
		else
			echo "line$i"
		fi
	done >file.txt &&
	git diff -U1 >out &&
	grep "line3" out &&
	grep "line17" out
'

test_expect_success '-U with --cached' '
	cd repo &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "STAGED"; else echo "line$i"; fi
	done >file.txt &&
	git add file.txt &&
	git diff --cached -U1 >out &&
	grep "line10" out
'

test_expect_success 'setup for -U with format options' '
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq 1 20); do echo "line$i"; done >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt
'

test_expect_success '-U does not affect --exit-code' '
	cd repo2 &&
	test_must_fail git diff -U0 --exit-code
'

test_expect_success '-U does not affect --quiet' '
	cd repo2 &&
	test_must_fail git diff -U0 --quiet
'

test_expect_success '-U does not affect --name-only' '
	cd repo2 &&
	git diff -U0 --name-only >out &&
	grep "file\.txt" out
'

test_expect_success '-U does not affect --name-status' '
	cd repo2 &&
	git diff -U0 --name-status >out &&
	grep "file\.txt" out
'

test_expect_success '-U does not affect --stat' '
	cd repo2 &&
	git diff -U0 --stat >out &&
	grep "file\.txt" out
'

test_expect_success '-U does not affect --numstat' '
	cd repo2 &&
	git diff -U0 --numstat >out &&
	grep "file\.txt" out
'

# --- additional context tests ---

test_expect_success '-U0 has no context lines (only +/- and @@)' '
	cd repo2 &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U0 >out &&
	! grep "^  *line" out
'

test_expect_success '-U3 default matches no -U option' '
	cd repo2 &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff >default_out &&
	git diff -U3 >u3_out &&
	test_cmp default_out u3_out
'

test_expect_success '-U20 covers all lines of 20-line file' '
	cd repo2 &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "CHANGED"; else echo "line$i"; fi
	done >file.txt &&
	git diff -U20 >out &&
	grep "line1" out &&
	grep "line20" out
'

test_expect_success 'context between commits' '
	git init repo3 &&
	cd repo3 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq 1 20); do echo "line$i"; done >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	for i in $(seq 1 20); do
		if test "$i" = "10"; then echo "MOD"; else echo "line$i"; fi
	done >file.txt &&
	git add file.txt &&
	git commit -m "modify" &&
	c1=$(git rev-parse HEAD~1) && c2=$(git rev-parse HEAD) &&
	git diff -U1 "$c1" "$c2" >out &&
	grep "line10" out &&
	grep "MOD" out
'

test_expect_success '-U0 between commits has no context' '
	cd repo3 &&
	c1=$(git rev-parse HEAD~1) && c2=$(git rev-parse HEAD) &&
	git diff -U0 "$c1" "$c2" >out &&
	grep "^-line10" out &&
	grep "^+MOD" out
'

test_expect_success '-U0 with --cached' '
	cd repo3 &&
	for i in $(seq 1 20); do
		if test "$i" = "15"; then echo "STAGED"; else echo "line$i"; fi
	done >file.txt &&
	git add file.txt &&
	git diff -U0 --cached >out &&
	grep "STAGED" out &&
	git reset HEAD -- file.txt &&
	git checkout -- file.txt
'

test_expect_success 'context with two hunks merging at large -U' '
	git init repo4 &&
	cd repo4 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq 1 30); do echo "line$i"; done >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	for i in $(seq 1 30); do
		if test "$i" = "5" || test "$i" = "25"; then
			echo "CHANGED$i"
		else
			echo "line$i"
		fi
	done >file.txt &&
	git add file.txt &&
	git commit -m "modify" &&
	c1=$(git rev-parse HEAD~1) && c2=$(git rev-parse HEAD) &&
	git diff -U0 "$c1" "$c2" >out_u0 &&
	test $(grep -c "^@@" out_u0) -ge 2 &&
	git diff -U100 "$c1" "$c2" >out_u100 &&
	test $(grep -c "^@@" out_u100) = 1
'

test_done

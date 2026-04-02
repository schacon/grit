#!/bin/sh
# Test grit ls-files with ignore/exclude patterns and check-ignore.

test_description='grit ls-files exclude patterns and check-ignore'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup repo with .gitignore' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	cat >.gitignore <<-\EOF &&
	*.log
	*.tmp
	build/
	EOF
	echo "tracked" >tracked.txt &&
	echo "also tracked" >src.c &&
	git add .gitignore tracked.txt src.c &&
	git commit -m "initial"
'

test_expect_success 'check-ignore identifies ignored file by extension' '
	cd repo &&
	echo "log data" >debug.log &&
	grit check-ignore debug.log >actual &&
	echo "debug.log" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore identifies ignored tmp file' '
	cd repo &&
	echo "temp" >scratch.tmp &&
	grit check-ignore scratch.tmp >actual &&
	echo "scratch.tmp" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore identifies ignored directory' '
	cd repo &&
	mkdir -p build &&
	echo "object" >build/main.o &&
	grit check-ignore build/main.o >actual &&
	echo "build/main.o" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore returns 1 for non-ignored file' '
	cd repo &&
	test_must_fail grit check-ignore tracked.txt
'

test_expect_success 'check-ignore -v shows source of ignore rule' '
	cd repo &&
	grit check-ignore -v debug.log >actual &&
	grep ".gitignore" actual &&
	grep "\\*.log" actual &&
	grep "debug.log" actual
'

test_expect_success 'check-ignore -v shows line number' '
	cd repo &&
	grit check-ignore -v debug.log >actual &&
	grep ".gitignore:1:" actual
'

test_expect_success 'check-ignore -v for build dir shows correct rule' '
	cd repo &&
	grit check-ignore -v build/main.o >actual &&
	grep ".gitignore:3:" actual &&
	grep "build/" actual
'

test_expect_success 'check-ignore with multiple paths' '
	cd repo &&
	grit check-ignore debug.log scratch.tmp build/main.o >actual &&
	test_line_count = 3 actual &&
	grep "debug.log" actual &&
	grep "scratch.tmp" actual &&
	grep "build/main.o" actual
'

test_expect_success 'check-ignore --stdin reads paths from stdin' '
	cd repo &&
	printf "debug.log\nscratch.tmp\ntracked.txt\n" |
	grit check-ignore --stdin >actual &&
	test_line_count = 2 actual &&
	grep "debug.log" actual &&
	grep "scratch.tmp" actual &&
	! grep "tracked.txt" actual
'

test_expect_success 'check-ignore with nested .gitignore' '
	cd repo &&
	mkdir -p subdir &&
	echo "*.dat" >subdir/.gitignore &&
	echo "data" >subdir/test.dat &&
	echo "code" >subdir/test.c &&
	$REAL_GIT add subdir/.gitignore subdir/test.c &&
	$REAL_GIT commit -m "add subdir gitignore" &&
	grit check-ignore subdir/test.dat >actual &&
	echo "subdir/test.dat" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore -v shows nested .gitignore source' '
	cd repo &&
	grit check-ignore -v subdir/test.dat >actual &&
	grep "subdir/.gitignore" actual
'

test_expect_success 'check-ignore with negation pattern' '
	cd repo &&
	cat >.gitignore <<-\EOF &&
	*.log
	!important.log
	*.tmp
	build/
	EOF
	$REAL_GIT add .gitignore &&
	$REAL_GIT commit -m "add negation" &&
	test_must_fail grit check-ignore important.log
'

test_expect_success 'check-ignore still ignores non-negated log files' '
	cd repo &&
	grit check-ignore debug.log >actual &&
	echo "debug.log" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files -c shows cached (tracked) files' '
	cd repo &&
	grit ls-files -c >actual &&
	grep "tracked.txt" actual &&
	grep "src.c" actual &&
	grep ".gitignore" actual
'

test_expect_success 'ls-files -c does not show untracked files' '
	cd repo &&
	echo "untracked" >not-added.txt &&
	grit ls-files -c >actual &&
	! grep "not-added.txt" actual
'

test_expect_success 'ls-files with pathspec restricts output' '
	cd repo &&
	grit ls-files subdir/ >actual &&
	grep "subdir/" actual &&
	! grep "tracked.txt" actual
'

test_expect_success 'ls-files -s shows staged info' '
	cd repo &&
	grit ls-files -s >actual &&
	grep "^100644" actual &&
	grep "tracked.txt" actual
'

test_expect_success 'setup repo with gitignore wildcards' '
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	cat >.gitignore <<-\EOF &&
	*.o
	*.a
	*.so
	__pycache__/
	*.pyc
	EOF
	echo "code" >main.c &&
	git add .gitignore main.c &&
	git commit -m "initial"
'

test_expect_success 'check-ignore matches object files' '
	cd repo2 &&
	echo obj >main.o &&
	grit check-ignore main.o >actual &&
	echo "main.o" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore matches archive files' '
	cd repo2 &&
	echo lib >libfoo.a &&
	grit check-ignore libfoo.a >actual &&
	echo "libfoo.a" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore matches shared library files' '
	cd repo2 &&
	echo lib >libfoo.so &&
	grit check-ignore libfoo.so >actual &&
	echo "libfoo.so" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore matches __pycache__ directory' '
	cd repo2 &&
	mkdir -p __pycache__ &&
	echo cache >__pycache__/module.pyc &&
	grit check-ignore __pycache__/module.pyc >actual &&
	echo "__pycache__/module.pyc" >expect &&
	test_cmp expect actual
'

test_expect_success 'check-ignore does not match tracked .c file' '
	cd repo2 &&
	test_must_fail grit check-ignore main.c
'

test_done

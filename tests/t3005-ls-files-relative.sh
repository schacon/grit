#!/bin/sh
# Test grit ls-files behavior when run from subdirectories.

test_description='grit ls-files relative paths from subdirectories'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup repository with nested directories' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "root" >root.txt &&
	mkdir -p src/core src/util doc/api &&
	echo "main" >src/main.c &&
	echo "core" >src/core/engine.c &&
	echo "helper" >src/core/helper.c &&
	echo "util" >src/util/string.c &&
	echo "api" >doc/api/ref.md &&
	echo "guide" >doc/guide.md &&
	git add -A &&
	git commit -m "initial"
'

test_expect_success 'ls-files from root shows all files' '
	cd repo &&
	grit ls-files >actual &&
	test_line_count = 7 actual
'

test_expect_success 'ls-files from src/ shows relative paths' '
	cd repo/src &&
	grit ls-files >actual &&
	grep "main.c" actual &&
	grep "core/engine.c" actual &&
	grep "core/helper.c" actual &&
	grep "util/string.c" actual &&
	! grep "root.txt" actual &&
	! grep "doc/" actual
'

test_expect_success 'ls-files from src/core/ shows only core files' '
	cd repo/src/core &&
	grit ls-files >actual &&
	grep "engine.c" actual &&
	grep "helper.c" actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files from doc/ shows doc files' '
	cd repo/doc &&
	grit ls-files >actual &&
	grep "api/ref.md" actual &&
	grep "guide.md" actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files from doc/api/ shows only api files' '
	cd repo/doc/api &&
	grit ls-files >actual &&
	grep "ref.md" actual &&
	test_line_count = 1 actual
'

test_expect_success 'ls-files -s from subdirectory shows staging info' '
	cd repo/src &&
	grit ls-files -s >actual &&
	grep "^100644" actual &&
	grep "main.c" actual &&
	grep "core/engine.c" actual
'

test_expect_success 'ls-files with pathspec from subdirectory' '
	cd repo/src &&
	grit ls-files core/ >actual &&
	grep "core/engine.c" actual &&
	grep "core/helper.c" actual &&
	! grep "main.c" actual &&
	! grep "util/" actual
'

test_expect_success 'ls-files with file pathspec from subdirectory' '
	cd repo/src &&
	grit ls-files main.c >actual &&
	echo "main.c" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files -c from subdirectory' '
	cd repo/src/util &&
	grit ls-files -c >actual &&
	grep "string.c" actual &&
	test_line_count = 1 actual
'

test_expect_success 'setup repo with deeply nested paths' '
	cd repo &&
	mkdir -p a/b/c/d/e &&
	echo "deep" >a/b/c/d/e/leaf.txt &&
	echo "mid" >a/b/c/mid.txt &&
	git add -A &&
	git commit -m "add deep paths"
'

test_expect_success 'ls-files from deep directory shows only leaf' '
	cd repo/a/b/c/d/e &&
	grit ls-files >actual &&
	echo "leaf.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files from mid-level shows subtree' '
	cd repo/a/b/c &&
	grit ls-files >actual &&
	grep "d/e/leaf.txt" actual &&
	grep "mid.txt" actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files from a/ shows all nested files' '
	cd repo/a &&
	grit ls-files >actual &&
	grep "b/c/d/e/leaf.txt" actual &&
	grep "b/c/mid.txt" actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files -C flag changes working directory' '
	cd repo &&
	grit ls-files -C src/core >actual &&
	grep "engine.c" actual &&
	grep "helper.c" actual &&
	! grep "main.c" actual &&
	! grep "root.txt" actual
'

test_expect_success 'ls-files -C to deep subdirectory' '
	cd repo &&
	grit ls-files -C a/b/c/d/e >actual &&
	echo "leaf.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'setup modified and deleted files' '
	cd repo &&
	echo "modified" >src/main.c &&
	rm src/core/helper.c
'

test_expect_success 'ls-files -m from subdirectory shows modified' '
	cd repo/src &&
	grit ls-files -m >actual &&
	grep "main.c" actual
'

test_expect_success 'ls-files -d from subdirectory shows deleted' '
	cd repo/src &&
	grit ls-files -d >actual &&
	grep "core/helper.c" actual
'

test_expect_success 'restore and add untracked for ls-files -o' '
	cd repo &&
	git checkout -- . &&
	echo "new" >src/new-file.c &&
	echo "another" >src/core/bonus.c
'

test_expect_success 'ls-files matches git output from subdirectory' '
	cd repo/src &&
	grit ls-files -c | sort >grit_out &&
	$REAL_GIT ls-files | sort >git_out &&
	test_cmp git_out grit_out
'

test_expect_success 'ls-files -s from subdirectory matches git format' '
	cd repo/src/core &&
	grit ls-files -s >actual &&
	head -1 actual | grep -E "^[0-9]+ [0-9a-f]+ [0-9]"
'

test_expect_success 'ls-files --error-unmatch from subdirectory' '
	cd repo/src &&
	grit ls-files --error-unmatch main.c >actual &&
	grep "main.c" actual
'

test_expect_success 'ls-files --error-unmatch fails for missing file in subdir' '
	cd repo/src &&
	test_must_fail grit ls-files --error-unmatch nonexistent.c
'

test_done

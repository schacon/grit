#!/bin/sh
# Test grit ls-files with long paths, special characters, many files,
# and the -z NUL terminator option.

test_description='grit ls-files with long paths and special filenames'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup repo with deeply nested paths' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	mkdir -p a/b/c/d/e/f/g/h/i/j &&
	echo "deep" >a/b/c/d/e/f/g/h/i/j/leaf.txt &&
	echo "root" >root.txt &&
	git add -A &&
	git commit -m "initial"
'

test_expect_success 'ls-files shows deeply nested path' '
	cd repo &&
	grit ls-files >actual &&
	grep "a/b/c/d/e/f/g/h/i/j/leaf.txt" actual
'

test_expect_success 'ls-files -s shows full path with staging info' '
	cd repo &&
	grit ls-files -s >actual &&
	grep "a/b/c/d/e/f/g/h/i/j/leaf.txt" actual &&
	head -1 actual | grep "^100644"
'

test_expect_success 'setup repo with spaces in filenames' '
	git init repo-spaces &&
	cd repo-spaces &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "a" >"file with spaces.txt" &&
	echo "b" >"another file.doc" &&
	mkdir -p "dir with spaces" &&
	echo "c" >"dir with spaces/inner file.txt" &&
	git add -A &&
	git commit -m "spaces"
'

test_expect_success 'ls-files shows files with spaces' '
	cd repo-spaces &&
	grit ls-files >actual &&
	grep "file with spaces.txt" actual &&
	grep "another file.doc" actual &&
	grep "dir with spaces/inner file.txt" actual
'

test_expect_success 'ls-files -z uses NUL terminators' '
	cd repo-spaces &&
	grit ls-files -z >actual &&
	tr "\0" "\n" <actual >actual_lines &&
	grep "file with spaces.txt" actual_lines &&
	grep "another file.doc" actual_lines
'

test_expect_success 'ls-files -z output has no newlines in entries' '
	cd repo-spaces &&
	grit ls-files -z >actual &&
	count=$(tr "\0" "\n" <actual | grep -c ".") &&
	test "$count" -eq 3
'

test_expect_success 'setup repo with special characters' '
	git init repo-special &&
	cd repo-special &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "a" >"file-with-dashes.txt" &&
	echo "b" >"file_with_underscores.txt" &&
	echo "c" >"file.multiple.dots.txt" &&
	echo "d" >"CamelCase.TXT" &&
	echo "e" >"ALLCAPS.MD" &&
	echo "f" >"v2.0-release-notes.md" &&
	git add -A &&
	git commit -m "special chars"
'

test_expect_success 'ls-files shows special character filenames' '
	cd repo-special &&
	grit ls-files >actual &&
	grep "file-with-dashes.txt" actual &&
	grep "file_with_underscores.txt" actual &&
	grep "file.multiple.dots.txt" actual &&
	grep "CamelCase.TXT" actual &&
	grep "ALLCAPS.MD" actual &&
	grep "v2.0-release-notes.md" actual
'

test_expect_success 'ls-files lists files in sorted order' '
	cd repo-special &&
	grit ls-files >actual &&
	sort actual >sorted &&
	test_cmp sorted actual
'

test_expect_success 'setup repo with many files' '
	git init repo-many &&
	cd repo-many &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in $(seq -w 1 100); do
		echo "content $i" >file_$i.txt
	done &&
	git add -A &&
	git commit -m "100 files"
'

test_expect_success 'ls-files lists all 100 files' '
	cd repo-many &&
	grit ls-files >actual &&
	test_line_count = 100 actual
'

test_expect_success 'ls-files -s lists all 100 files with staging' '
	cd repo-many &&
	grit ls-files -s >actual &&
	test_line_count = 100 actual &&
	grep "^100644" actual | wc -l >count &&
	echo "100" >expect &&
	test_cmp expect count
'

test_expect_success 'ls-files matches git output for many files' '
	cd repo-many &&
	grit ls-files | sort >grit_out &&
	$REAL_GIT ls-files | sort >git_out &&
	test_cmp git_out grit_out
'

test_expect_success 'setup repo with long filename components' '
	git init repo-longname &&
	cd repo-longname &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	longname="this-is-a-very-long-filename-that-tests-the-limits-of-reasonable-path-lengths.txt" &&
	echo "long" >"$longname" &&
	git add -A &&
	git commit -m "long filename"
'

test_expect_success 'ls-files shows long filename' '
	cd repo-longname &&
	grit ls-files >actual &&
	grep "this-is-a-very-long-filename" actual
'

test_expect_success 'setup repo with mixed directory depths' '
	git init repo-mixed &&
	cd repo-mixed &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "1" >shallow.txt &&
	mkdir -p d1 &&
	echo "2" >d1/mid.txt &&
	mkdir -p d1/d2/d3 &&
	echo "3" >d1/d2/d3/deep.txt &&
	mkdir -p d1/d2/d3/d4/d5/d6 &&
	echo "4" >d1/d2/d3/d4/d5/d6/very-deep.txt &&
	git add -A &&
	git commit -m "mixed depths"
'

test_expect_success 'ls-files shows all depths' '
	cd repo-mixed &&
	grit ls-files >actual &&
	grep "^shallow.txt$" actual &&
	grep "^d1/mid.txt$" actual &&
	grep "^d1/d2/d3/deep.txt$" actual &&
	grep "^d1/d2/d3/d4/d5/d6/very-deep.txt$" actual &&
	test_line_count = 4 actual
'

test_expect_success 'ls-files with pathspec on deep directory' '
	cd repo-mixed &&
	grit ls-files d1/d2/d3/d4/ >actual &&
	grep "d1/d2/d3/d4/d5/d6/very-deep.txt" actual &&
	test_line_count = 1 actual
'

test_expect_success 'ls-files --error-unmatch on long path' '
	cd repo-mixed &&
	grit ls-files --error-unmatch d1/d2/d3/deep.txt >actual &&
	grep "d1/d2/d3/deep.txt" actual
'

test_expect_success 'ls-files --error-unmatch fails for wrong deep path' '
	cd repo-mixed &&
	test_must_fail grit ls-files --error-unmatch d1/d2/nonexistent.txt
'

test_expect_success 'ls-files -z with long paths has NUL separators' '
	cd repo-mixed &&
	grit ls-files -z >actual &&
	tr "\0" "\n" <actual >actual_lines &&
	test_line_count = 4 actual_lines
'

test_expect_success 'ls-files --deduplicate with long paths' '
	cd repo-mixed &&
	grit ls-files --deduplicate >actual &&
	sort -u actual >unique &&
	test_cmp actual unique
'

test_done

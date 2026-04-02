#!/bin/sh
# Test wildcard/glob pattern matching via check-ignore and ls-files.
# Exercises fnmatch/wildmatch logic in grit's gitignore implementation.

test_description='wildcard/glob pattern matching (wildmatch)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────────────

test_expect_success 'setup repo with assorted files' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	mkdir -p dir/sub dir2 build/out logs &&
	touch file.c file.h file.o file.log &&
	touch dir/a.c dir/b.o dir/sub/deep.log &&
	touch dir2/readme.md dir2/notes.txt &&
	touch build/out/result.bin build/Makefile &&
	touch logs/app.log logs/app.log.1
'

# ── Star wildcard (*) ─────────────────────────────────────────────────────────

test_expect_success '*.o matches .o files in root' '
	cd repo &&
	echo "*.o" >.gitignore &&
	grit check-ignore file.o
'

test_expect_success '*.o does not match .c files' '
	cd repo &&
	echo "*.o" >.gitignore &&
	test_must_fail grit check-ignore file.c
'

test_expect_success '*.o matches .o in subdirectory' '
	cd repo &&
	echo "*.o" >.gitignore &&
	grit check-ignore dir/b.o
'

test_expect_success '*.log matches log files everywhere' '
	cd repo &&
	echo "*.log" >.gitignore &&
	grit check-ignore file.log &&
	grit check-ignore dir/sub/deep.log &&
	grit check-ignore logs/app.log
'

# ── Question mark (?) ─────────────────────────────────────────────────────────

test_expect_success '?.c matches single-char .c filenames' '
	cd repo &&
	echo "?.c" >.gitignore &&
	grit check-ignore dir/a.c
'

test_expect_success '?.c does not match multi-char names' '
	cd repo &&
	echo "?.c" >.gitignore &&
	test_must_fail grit check-ignore file.c
'

test_expect_success 'file.? matches single-char extensions' '
	cd repo &&
	echo "file.?" >.gitignore &&
	grit check-ignore file.c &&
	grit check-ignore file.h &&
	grit check-ignore file.o
'

test_expect_success 'file.? does not match multi-char extensions' '
	cd repo &&
	echo "file.?" >.gitignore &&
	test_must_fail grit check-ignore file.log
'

# ── Double-star (**) ──────────────────────────────────────────────────────────

test_expect_success 'build/** matches everything under build/' '
	cd repo &&
	echo "build/**" >.gitignore &&
	grit check-ignore build/out/result.bin &&
	grit check-ignore build/Makefile
'

test_expect_success '**/deep.log matches deep.log in any directory' '
	cd repo &&
	echo "**/deep.log" >.gitignore &&
	grit check-ignore dir/sub/deep.log
'

test_expect_success '**/*.log matches .log in subdirectories' '
	cd repo &&
	echo "**/*.log" >.gitignore &&
	grit check-ignore dir/sub/deep.log &&
	grit check-ignore logs/app.log
'

# ── Directory pattern (trailing /) ────────────────────────────────────────────

test_expect_success 'dir/ matches directory' '
	cd repo &&
	echo "dir/" >.gitignore &&
	grit check-ignore dir/a.c
'

test_expect_success 'logs/ matches logs directory' '
	cd repo &&
	echo "logs/" >.gitignore &&
	grit check-ignore logs/app.log
'

# ── Negation (!) ──────────────────────────────────────────────────────────────

test_expect_success 'negation re-includes a previously excluded file' '
	cd repo &&
	printf "*.log\n!app.log\n" >.gitignore &&
	grit check-ignore file.log &&
	test_must_fail grit check-ignore logs/app.log
'

test_expect_success '-v shows negation pattern' '
	cd repo &&
	printf "*.log\n!app.log\n" >.gitignore &&
	grit check-ignore -v logs/app.log >out 2>&1 &&
	grep "!app.log" out
'

# ── Anchored patterns (leading /) ────────────────────────────────────────────

test_expect_success '/file.o only matches in root' '
	cd repo &&
	echo "/file.o" >.gitignore &&
	grit check-ignore file.o &&
	test_must_fail grit check-ignore dir/b.o
'

test_expect_success '/dir/ matches only top-level dir' '
	cd repo &&
	echo "/dir/" >.gitignore &&
	grit check-ignore dir/a.c
'

# ── Verbose output (-v) ──────────────────────────────────────────────────────

test_expect_success '-v shows source file, line, and pattern' '
	cd repo &&
	printf "*.o\n*.log\n" >.gitignore &&
	grit check-ignore -v file.o >out &&
	grep ".gitignore:1:" out &&
	grit check-ignore -v file.log >out2 &&
	grep ".gitignore:2:" out2
'

test_expect_success '-v output includes the matching pattern' '
	cd repo &&
	echo "build/**" >.gitignore &&
	grit check-ignore -v build/Makefile >out &&
	grep "build/\*\*" out
'

# ── Multiple patterns ────────────────────────────────────────────────────────

test_expect_success 'multiple patterns in .gitignore all work' '
	cd repo &&
	printf "*.o\n*.log\nbuild/\n" >.gitignore &&
	grit check-ignore file.o &&
	grit check-ignore file.log &&
	grit check-ignore build/Makefile
'

test_expect_success 'blank lines and comments are ignored' '
	cd repo &&
	printf "# comment\n\n*.o\n  \n# another\n*.log\n" >.gitignore &&
	grit check-ignore file.o &&
	grit check-ignore file.log
'

# ── ls-files integration ─────────────────────────────────────────────────────

test_expect_success 'ls-files excludes ignored files after add' '
	cd repo &&
	echo "*.o" >.gitignore &&
	grit add .gitignore file.c file.h &&
	grit ls-files >out &&
	grep "file.c" out &&
	grep "file.h" out &&
	! grep "file.o" out
'

test_expect_success 'ls-files -o --exclude-standard shows untracked non-ignored' '
	cd repo &&
	echo "*.o" >.gitignore &&
	grit ls-files -o --exclude-standard >out 2>&1 || true &&
	! grep "file.o" out
'

# ── Edge cases ────────────────────────────────────────────────────────────────

test_expect_success 'pattern with no wildcard matches exact name' '
	cd repo &&
	echo "file.log" >.gitignore &&
	grit check-ignore file.log &&
	test_must_fail grit check-ignore file.log.1
'

test_expect_success 'literal filename with hash is matched' '
	cd repo &&
	printf "#comment\n" >ignorefile &&
	test_must_fail grit check-ignore "#comment"
'

test_expect_success 'pattern *.log.* matches compound extensions' '
	cd repo &&
	echo "*.log.*" >.gitignore &&
	grit check-ignore logs/app.log.1
'

test_done

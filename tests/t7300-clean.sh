#!/bin/sh

test_description='git clean basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init clean-repo &&
	cd clean-repo &&
	git config clean.requireForce no &&
	mkdir -p src &&
	touch src/part1.c Makefile &&
	echo build >.gitignore &&
	echo "*.o" >>.gitignore &&
	git add . &&
	git commit -m setup &&
	touch src/part2.c README &&
	git add .
'

test_expect_success 'git clean removes untracked files' '
	cd clean-repo &&
	mkdir -p build docs &&
	touch a.out src/part3.c docs/manual.txt obj.o build/lib.so &&
	git clean &&
	test_path_is_file Makefile &&
	test_path_is_file README &&
	test_path_is_file src/part1.c &&
	test_path_is_file src/part2.c &&
	test_path_is_missing a.out &&
	test_path_is_missing src/part3.c &&
	test_path_is_file docs/manual.txt &&
	test_path_is_file obj.o &&
	test_path_is_file build/lib.so
'

test_expect_success 'git clean src/' '
	cd clean-repo &&
	mkdir -p build docs &&
	touch a.out src/part3.c docs/manual.txt obj.o build/lib.so &&
	git clean src/ &&
	test_path_is_file a.out &&
	test_path_is_missing src/part3.c
'

test_expect_success 'git clean -d removes directories' '
	cd clean-repo &&
	mkdir -p build docs src/feature &&
	touch a.out src/part3.c src/feature/file.c docs/manual.txt obj.o build/lib.so &&
	git clean -d &&
	test_path_is_missing docs &&
	test_path_is_file Makefile
'

test_expect_success 'git clean -n dry run' '
	cd clean-repo &&
	touch dry-run-file &&
	git clean -n >output &&
	test_path_is_file dry-run-file &&
	grep "dry-run-file" output &&
	rm -f dry-run-file
'

test_expect_success 'git clean -x removes ignored files' '
	cd clean-repo &&
	mkdir -p build &&
	touch obj.o build/lib.so untracked.c &&
	git clean -x -d &&
	test_path_is_missing obj.o &&
	test_path_is_missing build &&
	test_path_is_missing untracked.c
'

test_expect_success 'git clean -X removes only ignored files' '
	cd clean-repo &&
	mkdir -p build &&
	touch obj.o build/lib.so untracked.c &&
	git clean -X -d &&
	test_path_is_missing obj.o &&
	test_path_is_missing build &&
	test_path_is_file untracked.c &&
	rm -f untracked.c
'

test_expect_success 'git clean -f force mode' '
	cd clean-repo &&
	git config clean.requireForce true &&
	touch force-file &&
	git clean -f &&
	test_path_is_missing force-file &&
	git config clean.requireForce no
'

test_expect_success 'git clean -q quiet mode' '
	cd clean-repo &&
	touch quiet-file &&
	git clean -q >output &&
	test_path_is_missing quiet-file &&
	test_must_be_empty output
'

test_expect_success 'git clean -f -d removes nested untracked directories' '
	cd clean-repo &&
	mkdir -p nested/deep/dir &&
	touch nested/deep/dir/file.txt &&
	git clean -f -d &&
	test_path_is_missing nested
'

test_expect_success 'git clean -x -d removes ignored files and directories' '
	cd clean-repo &&
	mkdir -p build/output &&
	touch build/output/result.o &&
	touch extra.o &&
	git clean -x -d &&
	test_path_is_missing build &&
	test_path_is_missing extra.o
'

test_expect_success 'nested bare repositories should be cleaned with -f -d' '
	cd clean-repo &&
	rm -fr strange_bare &&
	mkdir strange_bare &&
	git init --bare strange_bare/.git &&
	git clean -f -d &&
	test_path_is_missing strange_bare
'

test_done

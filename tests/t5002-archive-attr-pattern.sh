#!/bin/sh

test_description='git archive with path patterns'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "readme" >README.md &&
	echo "code" >main.c &&
	echo "header" >main.h &&
	mkdir src &&
	echo "source" >src/util.c &&
	echo "header" >src/util.h &&
	mkdir docs &&
	echo "doc" >docs/guide.txt &&
	git add README.md main.c main.h src docs &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'archive single file path' '
	git archive HEAD -- README.md >single.tar &&
	mkdir -p single-extract &&
	(cd single-extract && tar xf ../single.tar) &&
	test_path_is_file single-extract/README.md &&
	test_path_is_missing single-extract/main.c
'

test_expect_success 'archive directory path' '
	git archive HEAD -- src >dir.tar &&
	mkdir -p dir-extract &&
	(cd dir-extract && tar xf ../dir.tar) &&
	test_path_is_file dir-extract/src/util.c &&
	test_path_is_file dir-extract/src/util.h &&
	test_path_is_missing dir-extract/main.c
'

test_expect_success 'archive multiple paths' '
	git archive HEAD -- README.md docs >multi.tar &&
	mkdir -p multi-extract &&
	(cd multi-extract && tar xf ../multi.tar) &&
	test_path_is_file multi-extract/README.md &&
	test_path_is_file multi-extract/docs/guide.txt &&
	test_path_is_missing multi-extract/main.c
'

test_expect_success 'archive full with prefix' '
	git archive --prefix=proj/ HEAD >prefix-full.tar &&
	mkdir -p pf-extract &&
	(cd pf-extract && tar xf ../prefix-full.tar) &&
	test_path_is_file pf-extract/proj/src/util.c &&
	test_path_is_file pf-extract/proj/README.md
'

test_done

#!/bin/sh

test_description='test corner cases of git-archive'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo content >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'tar archive of commit' '
	cd repo &&
	git archive --format=tar HEAD >archive.tar &&
	mkdir extract &&
	tar xf archive.tar -C extract &&
	echo content >expect &&
	test_cmp expect extract/file
'

test_expect_success 'tar archive with prefix' '
	cd repo &&
	rm -rf extract &&
	git archive --format=tar --prefix=foo/ HEAD >prefix.tar &&
	mkdir extract &&
	tar xf prefix.tar -C extract &&
	echo content >expect &&
	test_cmp expect extract/foo/file
'

if command -v unzip >/dev/null 2>&1; then
	test_set_prereq UNZIP
fi

test_expect_success UNZIP 'zip archive of commit' '
	cd repo &&
	git archive --format=zip -o archive.zip HEAD &&
	test -f archive.zip &&
	rm -rf extract &&
	mkdir extract &&
	(cd extract && unzip -o ../archive.zip) &&
	echo content >expect &&
	test_cmp expect extract/file
'

test_expect_success 'archive with specific path' '
	cd repo &&
	echo other >other &&
	git add other &&
	git commit -m "add other" &&
	git archive --format=tar HEAD file >specific.tar &&
	rm -rf extract &&
	mkdir extract &&
	tar xf specific.tar -C extract &&
	test_cmp expect extract/file &&
	test_path_is_missing extract/other
'

test_expect_success 'archive output to file via -o' '
	cd repo &&
	git archive --format=tar -o output.tar HEAD &&
	test -f output.tar &&
	rm -rf extract &&
	mkdir extract &&
	tar xf output.tar -C extract &&
	test -f extract/file
'

test_done

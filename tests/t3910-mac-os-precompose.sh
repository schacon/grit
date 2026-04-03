#!/bin/sh
#
# Copyright (c) 2012 Torsten Bögershausen
#

test_description='utf-8 decomposed (nfd) converted to precomposed (nfc)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Basic test that grit handles UTF-8 filenames
test_expect_success 'setup repo' '
	git init repo &&
	cd repo
'

test_expect_success 'commit with ascii filename' '
	cd repo &&
	echo content >file.txt &&
	git add file.txt &&
	git commit -m "add ascii file" &&
	git ls-files >actual &&
	grep "file.txt" actual
'

test_expect_success 'commit with utf-8 content in message' '
	cd repo &&
	echo more >file2.txt &&
	git add file2.txt &&
	git commit -m "Ändere Datei" &&
	git log --max-count=1 --format="%s" >actual &&
	grep "ndere" actual
'

test_done

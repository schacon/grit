#!/bin/sh
# Tests for grit unpack-file.

test_description='grit unpack-file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	grit init repo &&
	cd repo
'

test_expect_success 'unpack-file writes blob content to a temp file' '
	echo "hello unpack" >src.txt &&
	oid=$(grit hash-object -w src.txt) &&
	tmppath=$(grit unpack-file "$oid") &&
	test -f "$tmppath" &&
	test "$(cat "$tmppath")" = "hello unpack" &&
	rm -f "$tmppath"
'

test_expect_success 'temp file is named .merge_file_*' '
	echo "test content" >blob.txt &&
	oid=$(grit hash-object -w blob.txt) &&
	tmppath=$(grit unpack-file "$oid") &&
	case "$tmppath" in
		*/.merge_file_*) : ;;
		*) echo "unexpected temp file name: $tmppath"; false ;;
	esac &&
	rm -f "$tmppath"
'

test_expect_success 'unpack-file fails for unknown OID' '
	test_must_fail grit unpack-file 0000000000000000000000000000000000000000
'

test_done

#!/bin/sh
# Tests for grit prune-packed: remove loose objects that are already in a pack.

test_description='prune-packed removes objects already in pack files'

. ./test-lib.sh

test_expect_success 'setup: create loose object' '
	git init repo &&
	cd repo &&
	BLOB=$(echo "hello prune" | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'prune-packed with no packs leaves loose object intact' '
	cd repo &&
	BLOB=$(echo "hello prune" | git hash-object --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	grit prune-packed &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'prune-packed --dry-run with no packs produces no output' '
	cd repo &&
	grit prune-packed --dry-run >out &&
	test_must_be_empty out
'

test_expect_success 'prune-packed -n is alias for --dry-run' '
	cd repo &&
	grit prune-packed -n >out &&
	test_must_be_empty out
'

test_expect_success 'prune-packed -q runs without error' '
	cd repo &&
	grit prune-packed -q
'

test_done

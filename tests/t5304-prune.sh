#!/bin/sh
# Ported subset from git/t/t5304-prune.sh focused on count-objects output.

test_description='count-objects loose count and verbose garbage accounting'

. ./test-lib.sh

test_expect_success 'count-objects loose count changes with hash-object -w' '
	grit init repo &&
	cd repo &&
	before=$(git count-objects | sed "s/ .*//") &&
	BLOB=$(echo aleph_0 | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	after=$(git count-objects | sed "s/ .*//") &&
	test $((before + 1)) = "$after" &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'count-objects -v reports garbage files' '
	cd repo &&
	mkdir -p .git/objects/pack &&
	>.git/objects/pack/fake.bar &&
	git count-objects -v >actual &&
	grep "^garbage: 1\$" actual
'

test_done

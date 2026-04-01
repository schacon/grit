#!/bin/sh
# Ported subset of git show-index functionality.

test_description='show-index basic behavior on generated pack index'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

test_expect_success 'setup packed repository fixture' '
	grit init repo &&
	cd repo &&
	echo hello >a.txt &&
	"$REAL_GIT" update-index --add a.txt &&
	tree=$("$REAL_GIT" write-tree) &&
	commit=$(echo "initial" | "$REAL_GIT" commit-tree "$tree") &&
	"$REAL_GIT" update-ref HEAD "$commit" &&
	"$REAL_GIT" repack -a -d &&
	idx=$(echo .git/objects/pack/*.idx) &&
	test_path_is_file "$idx"
'

test_expect_success 'show-index v2: output has offset oid (crc32) lines' '
	cd repo &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" >out &&
	test_line_count -gt 0 out &&
	# Every line must match: decimal offset, space, 40-hex OID, space, (8hexdigits)
	while IFS= read -r line; do
		echo "$line" | grep -qE "^[0-9]+ [0-9a-f]{40} \([0-9a-f]{8}\)$" ||
			{ echo "unexpected line: $line"; return 1; }
	done <out
'

test_expect_success 'show-index: --object-format=sha1 is accepted' '
	cd repo &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index --object-format=sha1 <"$idx" >out2 &&
	test_line_count -gt 0 out2
'

test_expect_success 'show-index: --object-format=sha256 is rejected' '
	cd repo &&
	idx=$(echo .git/objects/pack/*.idx) &&
	test_must_fail git show-index --object-format=sha256 <"$idx"
'

test_expect_success 'show-index: object IDs present in pack also appear in output' '
	cd repo &&
	idx=$(echo .git/objects/pack/*.idx) &&
	# Extract OIDs listed by verify-pack
	"$REAL_GIT" verify-pack -v "$idx" |
		grep -E "^[0-9a-f]{40}" |
		awk "{print \$1}" | sort >expected_oids &&
	# Extract OIDs from show-index output
	git show-index <"$idx" | awk "{print \$2}" | sort >actual_oids &&
	test_cmp expected_oids actual_oids
'

test_done

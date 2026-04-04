#!/bin/sh
#
# Upstream: t9303-fast-import-compression.sh
# Ported from git/t/t9303-fast-import-compression.sh for grit.
#

test_description='compression setting of fast-import utility'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Initialize a repo in the trash directory
git init --quiet

test_file_size () {
	wc -c <"$1" | tr -d ' '
}

import_large () {
	(
		echo blob
		echo "data <<EOD"
		printf "%2000000s\n" "$*"
		echo EOD
	) | git "$@" fast-import
}

while read expect config
do
	test_expect_success "fast-import (packed) with $config" '
		test_when_finished "rm -f .git/objects/pack/pack-*.*" &&
		test_when_finished "rm -rf .git/objects/??" &&
		import_large -c fastimport.unpacklimit=0 $config &&
		sz=$(test_file_size .git/objects/pack/pack-*.pack) &&
		case "$expect" in
		small) test "$sz" -le 100000 ;;
		large) test "$sz" -ge 100000 ;;
		esac
	'
done <<\EOF
large -c core.compression=0
small -c core.compression=9
large -c core.compression=0 -c pack.compression=0
large -c core.compression=9 -c pack.compression=0
small -c core.compression=0 -c pack.compression=9
small -c core.compression=9 -c pack.compression=9
large -c pack.compression=0
small -c pack.compression=9
EOF

while read expect config
do
	test_expect_success "fast-import (loose) with $config" '
		test_when_finished "rm -f .git/objects/pack/pack-*.*" &&
		test_when_finished "rm -rf .git/objects/??" &&
		import_large -c fastimport.unpacklimit=9 $config &&
		sz=$(test_file_size .git/objects/??/????*) &&
		case "$expect" in
		small) test "$sz" -le 100000 ;;
		large) test "$sz" -ge 100000 ;;
		esac
	'
done <<\EOF
large -c core.compression=0
small -c core.compression=9
large -c core.compression=0 -c core.loosecompression=0
large -c core.compression=9 -c core.loosecompression=0
small -c core.compression=0 -c core.loosecompression=9
small -c core.compression=9 -c core.loosecompression=9
large -c core.loosecompression=0
small -c core.loosecompression=9
EOF

test_done

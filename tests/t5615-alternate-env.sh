#!/bin/sh

test_description='handling of alternates in environment variables'

. ./test-lib.sh

test_expect_success 'setup' '
	git init
'

test_expect_success 'create alternate repositories' '
	git init --bare one.git &&
	one=$(echo one | git -C one.git hash-object -w --stdin) &&
	echo "$one" >one_oid &&
	git init --bare two.git &&
	two=$(echo two | git -C two.git hash-object -w --stdin) &&
	echo "$two" >two_oid
'

test_expect_success 'objects inaccessible without alternates' '
	one=$(cat one_oid) &&
	echo "$one missing" >expect &&
	echo "$one" | git cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

test_expect_success 'access alternate via absolute path' '
	one=$(cat one_oid) &&
	echo "$one blob" >expect &&
	echo "$one" | GIT_ALTERNATE_OBJECT_DIRECTORIES="$PWD/one.git/objects" \
		git cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

test_expect_success 'access multiple alternates' '
	one=$(cat one_oid) &&
	two=$(cat two_oid) &&
	cat >expect <<-EOF &&
	$one blob
	$two blob
	EOF
	printf "%s\n%s\n" "$one" "$two" | \
		GIT_ALTERNATE_OBJECT_DIRECTORIES="$PWD/one.git/objects:$PWD/two.git/objects" \
		git cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

test_done

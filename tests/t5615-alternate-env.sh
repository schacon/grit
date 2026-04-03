#!/bin/sh

test_description='handling of alternates in environment variables'

. ./test-lib.sh

test_expect_success 'setup' '
	git init
'

test_expect_success 'create alternate repositories' '
	git init --bare one.git &&
	one=$(echo one | git -C one.git hash-object -w --stdin) &&
	git init --bare two.git &&
	two=$(echo two | git -C two.git hash-object -w --stdin)
'

test_expect_success 'objects inaccessible without alternates' '
	echo "$one missing" >expect &&
	echo "$one" | git cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

test_expect_failure 'access alternate via absolute path' '
	echo "$one blob" >expect &&
	echo "$one" | GIT_ALTERNATE_OBJECT_DIRECTORIES="$PWD/one.git/objects" \
		git cat-file --batch-check="%(objectname) %(objecttype)" >actual &&
	test_cmp expect actual
'

test_expect_failure 'access multiple alternates' '
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

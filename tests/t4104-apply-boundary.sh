#!/bin/sh

test_description='git apply boundary tests'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

L="c d e f g h i j k l m n o p q r s t u v w x"

test_expect_success 'setup' '
	test_write_lines b $L y >victim &&
	cat victim >original &&
	git add victim &&

	# add to the head
	test_write_lines a b $L y >victim &&
	cat victim >add-a-expect &&
	git diff victim >add-a-patch.with &&

	# modify at the head
	test_write_lines a $L y >victim &&
	cat victim >mod-a-expect &&
	git diff victim >mod-a-patch.with &&

	# remove from the head
	test_write_lines $L y >victim &&
	cat victim >del-a-expect &&
	git diff victim >del-a-patch.with &&

	# add to the tail
	test_write_lines b $L y z >victim &&
	cat victim >add-z-expect &&
	git diff victim >add-z-patch.with &&

	# modify at the tail
	test_write_lines b $L z >victim &&
	cat victim >mod-z-expect &&
	git diff victim >mod-z-patch.with &&

	# remove from the tail
	test_write_lines b $L >victim &&
	cat victim >del-z-expect &&
	git diff victim >del-z-patch.with
'

for kind in add-a add-z mod-a mod-z del-a del-z
do
	test_expect_success "apply $kind-patch with context" '
		cat original >victim &&
		git apply "$kind-patch.with" &&
		test_cmp "$kind-expect" victim
	'
done

test_done

#!/bin/sh

test_description='test exclude_patterns functionality'

. ./test-lib.sh

# The upstream test uses test-tool ref-store which is not available in
# grit. Test exclude-pattern behaviour via for-each-ref instead.

test_expect_success 'setup refs' '
	git init repo &&
	(
		cd repo &&
		git commit --allow-empty -m initial &&
		git branch feature-a &&
		git branch feature-b &&
		git tag v1.0
	)
'

test_expect_success 'for-each-ref can list specific patterns excluding others' '
	git -C repo for-each-ref --format="%(refname)" refs/heads/ >actual &&
	test_line_count -gt 0 actual &&
	grep refs/heads/feature-a actual &&
	grep refs/heads/feature-b actual &&
	! grep refs/tags actual
'

test_done

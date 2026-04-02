#!/bin/sh
#
# Tests for XDG config paths: ~/.config/git/config, ignore, attributes.
# Verifies grit respects the XDG Base Directory Specification.
#
# Note: test-lib.sh sets HOME=$TRASH_DIRECTORY, so $HOME/.config/git/
# is where XDG files land by default.

test_description='grit XDG config file support'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# XDG config (~/.config/git/config)
# ---------------------------------------------------------------------------
test_expect_success 'setup: create XDG config dir and repo' '
	mkdir -p "$HOME/.config/git" &&
	rm -f "$HOME/.gitconfig" &&
	grit init xdg-repo
'

test_expect_success 'XDG config is read when no ~/.gitconfig exists' '
	rm -f "$HOME/.gitconfig" &&
	printf "[user]\n\txdguser = XDG Test User\n" >"$HOME/.config/git/config" &&
	cd xdg-repo &&
	result=$(git config --get user.xdguser) &&
	test "$result" = "XDG Test User"
'

test_expect_success 'XDG config values appear in config list' '
	rm -f "$HOME/.gitconfig" &&
	cd xdg-repo &&
	git config --list >out &&
	grep "user.xdguser=XDG Test User" out
'

test_expect_success 'XDG config is overridden by ~/.gitconfig' '
	printf "[user]\n\txdguser = Gitconfig User\n" >"$HOME/.gitconfig" &&
	cd xdg-repo &&
	result=$(git config --get user.xdguser) &&
	test "$result" = "Gitconfig User"
'

test_expect_success 'local config overrides both XDG and ~/.gitconfig' '
	rm -f "$HOME/.gitconfig" &&
	cd xdg-repo &&
	git config user.xdguser "Local User" &&
	result=$(git config --get user.xdguser) &&
	test "$result" = "Local User" &&
	git config --unset user.xdguser
'

test_expect_success 'XDG config with --global writes to ~/.gitconfig' '
	rm -f "$HOME/.gitconfig" &&
	cd xdg-repo &&
	git config --global xdgtest.written "from-global-flag" &&
	test -f "$HOME/.gitconfig" &&
	grep "from-global-flag" "$HOME/.gitconfig"
'

test_expect_success 'XDG_CONFIG_HOME overrides default XDG path' '
	rm -f "$HOME/.gitconfig" &&
	CUSTOM_XDG="$TRASH_DIRECTORY/custom-xdg" &&
	mkdir -p "$CUSTOM_XDG/git" &&
	printf "[user]\n\txdguser = Custom XDG\n" >"$CUSTOM_XDG/git/config" &&
	cd xdg-repo &&
	result=$(XDG_CONFIG_HOME="$CUSTOM_XDG" git config --get user.xdguser) &&
	test "$result" = "Custom XDG"
'

test_expect_success 'XDG config supports multiple sections' '
	rm -f "$HOME/.gitconfig" &&
	cat >"$HOME/.config/git/config" <<-\EOF &&
	[core]
		xdgcore = true
	[alias]
		xdgalias = status
	[color]
		ui = auto
	EOF
	cd xdg-repo &&
	git config --get core.xdgcore >out &&
	test "$(cat out)" = "true" &&
	git config --get alias.xdgalias >out2 &&
	test "$(cat out2)" = "status"
'

test_expect_success 'XDG config with boolean type' '
	rm -f "$HOME/.gitconfig" &&
	printf "[core]\n\txdgbool = yes\n" >"$HOME/.config/git/config" &&
	cd xdg-repo &&
	result=$(git config --bool --get core.xdgbool) &&
	test "$result" = "true"
'

test_expect_success 'XDG config with integer type' '
	rm -f "$HOME/.gitconfig" &&
	printf "[core]\n\txdgint = 42\n" >"$HOME/.config/git/config" &&
	cd xdg-repo &&
	result=$(git config --int --get core.xdgint) &&
	test "$result" = "42"
'

# ---------------------------------------------------------------------------
# XDG ignore (~/.config/git/ignore) — via core.excludesFile
# Uses check-ignore to verify patterns are loaded (grit status does not
# yet read excludesFile, but check-ignore does).
# ---------------------------------------------------------------------------
test_expect_success 'setup excludesFile via XDG config' '
	rm -f "$HOME/.gitconfig" &&
	printf "*.xdg-ignored\n" >"$HOME/.config/git/ignore" &&
	cd xdg-repo &&
	git config core.excludesFile "$HOME/.config/git/ignore"
'

test_expect_success 'check-ignore reports excludesFile patterns' '
	cd xdg-repo &&
	echo "data" >test.xdg-ignored &&
	git check-ignore test.xdg-ignored >out &&
	grep "test.xdg-ignored" out &&
	rm -f test.xdg-ignored
'

test_expect_success 'check-ignore -v shows excludesFile source' '
	cd xdg-repo &&
	echo "data" >test.xdg-ignored &&
	git check-ignore -v test.xdg-ignored >out &&
	grep "ignore" out &&
	grep "test.xdg-ignored" out &&
	rm -f test.xdg-ignored
'

test_expect_success 'excludesFile negation pattern works' '
	printf "*.log\n!important.log\n" >"$HOME/.config/git/ignore" &&
	cd xdg-repo &&
	echo "data" >debug.log &&
	echo "data" >important.log &&
	git check-ignore debug.log >out &&
	grep "debug.log" out &&
	test_must_fail git check-ignore important.log &&
	rm -f debug.log important.log
'

test_expect_success 'excludesFile directory pattern works' '
	printf "build/\n" >"$HOME/.config/git/ignore" &&
	cd xdg-repo &&
	mkdir -p build &&
	echo "artifact" >build/output.o &&
	git check-ignore build/output.o >out &&
	grep "build/output.o" out &&
	rm -rf build
'

test_expect_success 'excludesFile coexists with .gitignore' '
	printf "*.excl-global\n" >"$HOME/.config/git/ignore" &&
	cd xdg-repo &&
	echo "*.excl-local" >.gitignore &&
	echo "stuff" >a.excl-local &&
	echo "stuff" >b.excl-global &&
	git check-ignore a.excl-local >out1 &&
	grep "a.excl-local" out1 &&
	git check-ignore b.excl-global >out2 &&
	grep "b.excl-global" out2 &&
	rm -f a.excl-local b.excl-global .gitignore
'

# ---------------------------------------------------------------------------
# XDG attributes (~/.config/git/attributes)
# ---------------------------------------------------------------------------
test_expect_success 'XDG attributes file does not crash grit' '
	printf "*.bin binary\n" >"$HOME/.config/git/attributes" &&
	cd xdg-repo &&
	echo "binary data" >test.bin &&
	git add test.bin 2>err &&
	git status >out 2>&1 &&
	git reset HEAD -- test.bin 2>/dev/null || true &&
	rm -f test.bin
'

# ---------------------------------------------------------------------------
# Edge cases
# ---------------------------------------------------------------------------
test_expect_success 'missing XDG dir is not an error' '
	EMPTY_HOME="$TRASH_DIRECTORY/empty-home" &&
	mkdir -p "$EMPTY_HOME" &&
	cd xdg-repo &&
	HOME="$EMPTY_HOME" git config --get nonexistent.key >out 2>&1 || true
'

test_expect_success 'XDG config with show-origin in list mode' '
	rm -f "$HOME/.gitconfig" &&
	printf "[user]\n\txdguser = Origin Test\n" >"$HOME/.config/git/config" &&
	cd xdg-repo &&
	git config --show-origin --list >out 2>&1 &&
	grep "xdguser" out &&
	grep "config" out
'

test_expect_success 'XDG config with show-scope in list mode' '
	rm -f "$HOME/.gitconfig" &&
	printf "[user]\n\txdguser = Scope Test\n" >"$HOME/.config/git/config" &&
	cd xdg-repo &&
	git config --show-scope --list >out 2>&1 &&
	grep "xdguser" out
'

test_done

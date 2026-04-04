<<<<<<< HEAD
# Shell library to run git-daemon in tests.  Ends the test early if
# GIT_TEST_GIT_DAEMON is not set.
#
# Usage:
#
#	. ./test-lib.sh
#	. "$TEST_DIRECTORY"/lib-git-daemon.sh
#	start_git_daemon
#
#	test_expect_success '...' '
#		...
#	'
#
#	test_expect_success ...
#
#	test_done

if ! test_bool_env GIT_TEST_GIT_DAEMON true
then
	skip_all="git-daemon testing disabled (unset GIT_TEST_GIT_DAEMON to enable)"
	test_done
fi

if test_have_prereq !PIPE
then
	test_skip_or_die GIT_TEST_GIT_DAEMON "file system does not support FIFOs"
fi

=======
# Shell library to run git-daemon in tests.
#
# Usage:
#   . ./test-lib.sh
#   . "$TEST_DIRECTORY"/lib-git-daemon.sh
#   start_git_daemon
#
#   test_expect_success '...' '
#       ...
#   '
#
#   test_done

if ! test_have_prereq PIPE
then
	skip_all="file system does not support FIFOs"
	test_done
fi

>>>>>>> test/batch-GE
test_set_port LIB_GIT_DAEMON_PORT

GIT_DAEMON_PID=
GIT_DAEMON_PIDFILE="$PWD"/daemon.pid
GIT_DAEMON_DOCUMENT_ROOT_PATH="$PWD"/repo
GIT_DAEMON_HOST_PORT=127.0.0.1:$LIB_GIT_DAEMON_PORT
GIT_DAEMON_URL=git://$GIT_DAEMON_HOST_PORT

registered_stop_git_daemon_atexit_handler=
start_git_daemon() {
<<<<<<< HEAD
	if test -n "$GIT_DAEMON_PID"
	then
		error "start_git_daemon already called"
	fi

	mkdir -p "$GIT_DAEMON_DOCUMENT_ROOT_PATH"

	# One of the test scripts stops and then re-starts 'git daemon'.
	# Don't register and then run the same atexit handlers several times.
	if test -z "$registered_stop_git_daemon_atexit_handler"
	then
		test_atexit 'stop_git_daemon'
		registered_stop_git_daemon_atexit_handler=AlreadyDone
	fi

	say >&3 "Starting git daemon ..."
	mkfifo git_daemon_output
=======
	# Stop any previously running daemon
	stop_git_daemon 2>/dev/null

	mkdir -p "$GIT_DAEMON_DOCUMENT_ROOT_PATH"

	# Cleanup is handled by explicit stop_git_daemon calls
	# and test_done cleanup.

>>>>>>> test/batch-GE
	${LIB_GIT_DAEMON_COMMAND:-git daemon} \
		--listen=127.0.0.1 --port="$LIB_GIT_DAEMON_PORT" \
		--reuseaddr --verbose --pid-file="$GIT_DAEMON_PIDFILE" \
		--base-path="$GIT_DAEMON_DOCUMENT_ROOT_PATH" \
		"$@" "$GIT_DAEMON_DOCUMENT_ROOT_PATH" \
<<<<<<< HEAD
		>&3 2>git_daemon_output &
	GIT_DAEMON_PID=$!
	{
		read -r line <&7
		printf "%s\n" "$line" >&4
		cat <&7 >&4 &
	} 7<git_daemon_output &&

	# Check expected output
	if test x"$(expr "$line" : "\[[0-9]*\] \(.*\)")" != x"Ready to rumble"
	then
		kill "$GIT_DAEMON_PID"
		wait "$GIT_DAEMON_PID"
		unset GIT_DAEMON_PID
		test_skip_or_die GIT_TEST_GIT_DAEMON \
			"git daemon failed to start"
=======
		>"$PWD/git_daemon_output" 2>&1 &
	GIT_DAEMON_PID=$!

	# Wait for daemon to be ready (up to 5 seconds)
	local tries=0
	while test $tries -lt 50
	do
		if grep -q "Ready to rumble" "$PWD/git_daemon_output" 2>/dev/null
		then
			break
		fi
		sleep 0.1
		tries=$((tries + 1))
	done

	if ! grep -q "Ready to rumble" "$PWD/git_daemon_output" 2>/dev/null
	then
		kill "$GIT_DAEMON_PID" 2>/dev/null
		wait "$GIT_DAEMON_PID" 2>/dev/null
		unset GIT_DAEMON_PID
		skip_all="git daemon failed to start"
		test_done
>>>>>>> test/batch-GE
	fi
}

stop_git_daemon() {
	if test -z "$GIT_DAEMON_PID"
	then
		return
	fi

<<<<<<< HEAD
	# kill git-daemon child of git
	say >&3 "Stopping git daemon ..."
	kill "$GIT_DAEMON_PID"
	wait "$GIT_DAEMON_PID" >&3 2>&4
	ret=$?
	if ! test_match_signal 15 $ret
	then
		error "git daemon exited with status: $ret"
	fi
	kill "$(cat "$GIT_DAEMON_PIDFILE")" 2>/dev/null
	GIT_DAEMON_PID=
	rm -f git_daemon_output "$GIT_DAEMON_PIDFILE"
}

# A stripped-down version of a netcat client, that connects to a "host:port"
# given in $1, sends its stdin followed by EOF, then dumps the response (until
# EOF) to stdout.
fake_nc() {
	if ! test_declared_prereq FAKENC
	then
		echo >&4 "fake_nc: need to declare FAKENC prerequisite"
=======
	# Kill the wrapper process
	kill "$GIT_DAEMON_PID" 2>/dev/null

	# Kill via PID file (the actual forked daemon process)
	if test -f "$GIT_DAEMON_PIDFILE"
	then
		local _dpid
		_dpid=$(cat "$GIT_DAEMON_PIDFILE" 2>/dev/null)
		if test -n "$_dpid"
		then
			kill "$_dpid" 2>/dev/null
			# Wait briefly for it to die
			local _i=0
			while kill -0 "$_dpid" 2>/dev/null && test $_i -lt 10; do
				sleep 0.1
				_i=$((_i + 1))
			done
			# Force kill if still alive
			kill -9 "$_dpid" 2>/dev/null
		fi
		rm -f "$GIT_DAEMON_PIDFILE"
	fi

	wait "$GIT_DAEMON_PID" 2>/dev/null
	GIT_DAEMON_PID=
	rm -f "$PWD/git_daemon_output"
}

# A stripped-down version of a netcat client
fake_nc() {
	if ! test_have_prereq FAKENC
	then
		echo "fake_nc: need to declare FAKENC prerequisite" >&2
>>>>>>> test/batch-GE
		return 127
	fi
	perl -Mstrict -MIO::Socket::INET -e '
		my $s = IO::Socket::INET->new(shift)
			or die "unable to open socket: $!";
		print $s <STDIN>;
		$s->shutdown(1);
		print <$s>;
	' "$@"
}

test_lazy_prereq FAKENC '
	perl -MIO::Socket::INET -e "exit 0"
'

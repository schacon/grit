/* Small parent for full fork/exec/wait timing and per-child RSS/page faults.
 * cc -O2 grit-utils/examples/startup_measure.c -o /tmp/grit-startup-measure
 * Output: wall_ms peak_rss_kib minor_faults major_faults (Linux).
 */
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    struct timespec start, end;
    struct rusage usage;
    if (clock_gettime(CLOCK_MONOTONIC, &start)) return 3;
    pid_t pid = fork();
    if (pid < 0) return 3;
    if (pid == 0) {
        int fd = open("/dev/null", O_RDWR);
        if (fd < 0 || dup2(fd, 0) < 0 || dup2(fd, 1) < 0 || dup2(fd, 2) < 0)
            _exit(126);
        if (fd > 2) close(fd);
        execvp(argv[1], argv + 1);
        _exit(127);
    }
    int status;
    while (wait4(pid, &status, 0, &usage) < 0) {
        if (errno != EINTR) return 4;
    }
    if (clock_gettime(CLOCK_MONOTONIC, &end)) return 3;
    if (!WIFEXITED(status) || WEXITSTATUS(status)) return 5;
    printf("%.6f %ld %ld %ld\n",
           (end.tv_sec - start.tv_sec) * 1000.0 + (end.tv_nsec - start.tv_nsec) / 1000000.0,
           usage.ru_maxrss, usage.ru_minflt, usage.ru_majflt);
    return 0;
}

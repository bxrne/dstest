
#define _GNU_SOURCE
#include <time.h>
#include <sys/time.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdlib.h>

static int (*real_clock_gettime)(clockid_t, struct timespec *);
static time_t (*real_time)(time_t *);
static int (*real_gettimeofday)(struct timeval *, void *);
static const char *ctl_path;

__attribute__((constructor))
static void dstest_clock_init(void) {
    real_clock_gettime = dlsym(RTLD_NEXT, "clock_gettime");
    real_time = dlsym(RTLD_NEXT, "time");
    real_gettimeofday = dlsym(RTLD_NEXT, "gettimeofday");
    ctl_path = getenv("DSTEST_CLOCK_CTL");
}

static int read_clock(struct timespec *tp) {
    if (!ctl_path) return -1;
    int fd = open(ctl_path, O_RDONLY);
    if (fd < 0) return -1;
    long long nanos = -1;
    if (read(fd, &nanos, 8) != 8) { close(fd); return -1; }
    close(fd);
    if (nanos < 0) return -1;
    tp->tv_sec = (time_t)(nanos / 1000000000LL);
    tp->tv_nsec = (long)(nanos % 1000000000LL);
    return 0;
}

int clock_gettime(clockid_t clk, struct timespec *tp) {
    if (!real_clock_gettime) dstest_clock_init();
    if (clk == CLOCK_REALTIME
#ifdef CLOCK_REALTIME_COARSE
        || clk == CLOCK_REALTIME_COARSE
#endif
    ) {
        if (read_clock(tp) == 0) return 0;
    }
    return real_clock_gettime(clk, tp);
}

time_t time(time_t *t) {
    if (!real_time) dstest_clock_init();
    struct timespec tp;
    if (clock_gettime(CLOCK_REALTIME, &tp) == 0) {
        if (t) *t = tp.tv_sec;
        return tp.tv_sec;
    }
    return real_time(t);
}

int gettimeofday(struct timeval *__restrict tv, void *__restrict tz) {
    if (!real_gettimeofday) dstest_clock_init();
    struct timespec tp;
    /* glibc declares the parameter __nonnull; copy to a local so the
       defensive null check below does not trip -Wpointer-bool-conversion. */
    struct timeval *ptv = tv;
    if (ptv && clock_gettime(CLOCK_REALTIME, &tp) == 0) {
        ptv->tv_sec = (time_t)tp.tv_sec;
        ptv->tv_usec = (suseconds_t)(tp.tv_nsec / 1000);
        /* tz is obsolete and ignored; leaving it untouched matches the kernel. */
        return 0;
    }
    return real_gettimeofday(tv, tz);
}

// Started (by itself) in user and network namespaces of its own, its ids
// mapped to themselves, with CAP_SYS_CHROOT alone kept through the exec as
// an ambient capability; then chroots by a thread into its own
// /proc/thread-self/fdinfo, which vanishes as the thread ends, drops every
// capability, and looks at what is left.
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/capability.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int capset_(struct __user_cap_data_struct d[2]) {
  struct __user_cap_header_struct h = {_LINUX_CAPABILITY_VERSION_3, 0};
  return syscall(SYS_capset, &h, d);
}
static void caps(const char *when) {
  FILE *f = fopen("/proc/self/status", "r"); char line[256];
  printf("%s:", when);
  while (f && fgets(line, sizeof line, f))
    if (!strncmp(line, "Cap", 3)) { line[strcspn(line, "\n")] = 0; printf(" %s", line); }
  if (f) fclose(f); else printf(" (no /proc)");
  printf("\n");
}
static void write_file(const char *path, const char *text) {
  int fd = open(path, O_WRONLY); if (fd < 0 || write(fd, text, strlen(text)) < 0) { perror(path); _exit(90); } close(fd);
}
static void *chrooting(void *unused) {
  (void)unused;
  if (chroot("/proc/thread-self/fdinfo") != 0) { perror("chroot"); return (void *)1; }
  if (chdir("/") != 0) { perror("chdir"); return (void *)1; }
  return 0;
}
int main(int argc, char **argv) {
  if (argc > 1 && !strcmp(argv[1], "inside")) {
    caps("after exec");
    pthread_t t; void *r;
    pthread_create(&t, 0, chrooting, 0); pthread_join(t, &r);
    if (r) return 2;
    struct __user_cap_data_struct none[2] = {{0}};
    if (capset_(none) != 0) perror("capset");
    prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0);
    struct stat st;
    printf("stat /etc: %s\n", stat("/etc", &st) == 0 ? "there" : strerror(errno));
    printf("stat /proc: %s\n", stat("/proc", &st) == 0 ? "there" : strerror(errno));
    DIR *d = opendir("/"); int n = 0;
    if (d) { while (readdir(d)) n++; closedir(d); printf("opendir /: %d entries\n", n); }
    else printf("opendir /: %s\n", strerror(errno));
    char cwd[256]; printf("getcwd: %s\n", getcwd(cwd, sizeof cwd) ? cwd : strerror(errno));
    printf("chroot again: %s\n", chroot("/") == 0 ? "allowed" : strerror(errno));
    caps("at the end (read from /proc: gone)");
    return 0;
  }
  uid_t uid = getuid(); gid_t gid = getgid();
  pid_t pid = fork();
  if (pid == 0) {
    if (unshare(CLONE_NEWUSER | CLONE_NEWNET) != 0) { perror("unshare"); _exit(91); }
    char map[64];
    write_file("/proc/self/setgroups", "deny");
    snprintf(map, sizeof map, "%d %d 1", uid, uid); write_file("/proc/self/uid_map", map);
    snprintf(map, sizeof map, "%d %d 1", gid, gid); write_file("/proc/self/gid_map", map);
    // CAP_SYS_CHROOT alone, in the inheritable and ambient sets.
    struct __user_cap_header_struct h = {_LINUX_CAPABILITY_VERSION_3, 0};
    struct __user_cap_data_struct d[2];
    syscall(SYS_capget, &h, d);
    d[0].inheritable = 1u << CAP_SYS_CHROOT; d[1].inheritable = 0;
    if (capset_(d) != 0) { perror("capset inheritable"); _exit(92); }
    if (prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, CAP_SYS_CHROOT, 0, 0) != 0) { perror("ambient"); _exit(93); }
    execl("/proc/self/exe", argv[0], "inside", (char *)0);
    perror("exec"); _exit(94);
  }
  int status; waitpid(pid, &status, 0);
  printf("exited %d\n", WIFEXITED(status) ? WEXITSTATUS(status) : -1);
  return 0;
}

// Logs every getenv/secure_getenv a process makes (name, found or not) to
// $ENVPROBE_DIR/<kind>.<pid>, kind being "renderer" or "browser". The file
// is opened at load time, before a sandbox closes the file system.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int fd = -1;
static char *(*real_getenv)(const char *);
static char *(*real_secure_getenv)(const char *);

__attribute__((constructor)) static void init(void) {
  real_getenv = dlsym(RTLD_NEXT, "getenv");
  real_secure_getenv = dlsym(RTLD_NEXT, "secure_getenv");
  const char *dir = real_getenv("ENVPROBE_DIR");
  if (!dir) return;
  char cmd[4096] = {0};
  int c = open("/proc/self/cmdline", O_RDONLY);
  ssize_t n = c >= 0 ? read(c, cmd, sizeof cmd - 1) : 0;
  if (c >= 0) close(c);
  const char *kind = "browser";
  for (ssize_t i = 0; i < n; i += strlen(cmd + i) + 1)
    if (strncmp(cmd + i, "--type=renderer", 15) == 0) kind = "renderer";
  char path[4200];
  snprintf(path, sizeof path, "%s/%s.%d", dir, kind, getpid());
  fd = open(path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
}

static void note(const char *how, const char *name, const char *value) {
  if (fd < 0 || !name) return;
  char line[512];
  int len = snprintf(line, sizeof line, "%s %s %s\n", how, name, value ? "set" : "unset");
  if (len > 0) write(fd, line, len > (int)sizeof line ? (int)sizeof line : len);
}

char *getenv(const char *name) {
  if (!real_getenv) real_getenv = dlsym(RTLD_NEXT, "getenv");
  char *value = real_getenv(name);
  note("getenv", name, value);
  return value;
}

char *secure_getenv(const char *name) {
  if (!real_secure_getenv) real_secure_getenv = dlsym(RTLD_NEXT, "secure_getenv");
  char *value = real_secure_getenv(name);
  note("secure_getenv", name, value);
  return value;
}

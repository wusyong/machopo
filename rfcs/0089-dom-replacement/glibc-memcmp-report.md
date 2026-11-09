<!-- 給 glibc 的 bug 草稿（sourceware bugzilla，component: string）：2026-10-28 寫好，還沒送出，送出前要經過審核。照上游的語言寫成英文。找到的經過在 progress.md 的 1h-2。 -->

# x86_64: `__memcmp_evex_movbe` with a small size loads from both pointers even when the size is 0; the masked load into an unmapped page costs 100+ cycles on AMD Zen 5

**Version:** glibc 2.43 (Fedora 44, glibc-2.43-9.fc44.x86_64). **CPU:** AMD Ryzen AI 9 HX 370 (Zen 5). Linux 7.2.

## What happens

For sizes up to 32 bytes, `__memcmp_evex_movbe` (sysdeps/x86_64/multiarch/memcmp-evex-movbe.S) builds a mask with `bzhi` and does a masked load from each pointer:

```
cmp    $0x20,%rdx
ja     ...
mov    $0xffffffff,%eax
bzhi   %edx,%eax,%eax
kmovd  %eax,%k2
vmovdqu8 (%rsi),%ymm18{%k2}{z}
vpcmpnequb (%rdi),%ymm18,%k1{%k2}
```

This is correct: the masked-off bytes don't fault. But when the 32 bytes the load spans reach into an unmapped page, the CPU takes a fault-suppression assist. On Zen 5 that costs well over 100 cycles a call. Two common cases:

- **Size 0 with a dangling pointer.** The mask is all zeros, but both loads still happen. Rust compares empty slices with `memcmp(ptr, ptr, 0)`, and an empty slice's pointer is a dangling, aligned, non-null address: `0x1` for bytes, in page 0. Every comparison of two empty strings in Rust takes the assist (`""` as a hash map key, `s == ""`).
- **A short buffer ending just before an unmapped page.**

## Measurements

`memcmp` through a volatile function pointer, 10 million calls, TSC cycles per call, pinned to one Zen 5 core (repro below):

| Case | evex (default) | AVX2 (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX512F,-AVX512VL,-AVX512BW,-AVX512DQ,-AVX512CD`) |
| --- | ---: | ---: |
| n=0, both pointers mapped | 2.8–3.9 | 3.4 |
| n=0, one pointer `0x1` | 117–169 | 3.4 |
| n=0, both pointers `0x1` | 197–284 | 3.4 |
| n=2, mapped | 2.8–3.7 | 3.9 |
| n=2, 2 bytes before an unmapped page | 144–152 | 3.9 |

We found it in a browser engine: an attribute lookup that interned the empty namespace on every call spent 94% of its time in this instruction. The getter went from ~400 to ~84 cycles once we skipped the lookup of the empty string.

We have measured this only on Zen 5. We have not checked Zen 4 or Intel.

## Suggestion

- Return 0 early when the size is 0, before any load. It costs one predictable branch and avoids the assist for the dangling-pointer case.
- For small sizes, check whether the load would cross a page, as the AVX2 variant does, and take a non-masked path when it would.

## Repro

```c
// gcc -O2 -fno-builtin-memcmp memcmp0.c && taskset -c 0 ./a.out
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <x86intrin.h>

#define CALLS 10000000

static int (*volatile cmp)(const void *, const void *, size_t) = memcmp;

static double cycles_per_call(const void *a, const void *b, size_t n) {
    volatile int sink = 0;
    for (int i = 0; i < 1000; i++) sink += cmp(a, b, n);
    uint64_t start = __rdtsc();
    for (int i = 0; i < CALLS; i++) sink += cmp(a, b, n);
    uint64_t end = __rdtsc();
    (void)sink;
    return (double)(end - start) / CALLS;
}

int main(void) {
    static const char buffer[64] = "ab";
    const void *dangling = (const void *)1;  // an empty Rust slice's pointer
    char *pages = mmap(NULL, 8192, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    munmap(pages + 4096, 4096);
    char *page_end = pages + 4096 - 2;
    memcpy(page_end, "ab", 2);

    printf("n=0, both in mapped memory:    %6.1f\n", cycles_per_call(buffer, buffer + 8, 0));
    printf("n=0, one pointer 0x1:          %6.1f\n", cycles_per_call(dangling, buffer, 0));
    printf("n=0, both pointers 0x1:        %6.1f\n", cycles_per_call(dangling, dangling, 0));
    printf("n=2, mapped:                   %6.1f\n", cycles_per_call(buffer, buffer + 32, 2));
    printf("n=2, 2 bytes before unmapped:  %6.1f\n", cycles_per_call(page_end, buffer, 2));
    return 0;
}
```

<!-- 送出前再確認：sourceware bugzilla 上有沒有同一件事（搜 memcmp evex fault suppression / AMD），和最新的 glibc 是不是已經改了這段。 -->

"""Print a process's token from outside: groups (deny-only marked),
restricting SIDs, app container SID, integrity, default DACL."""

import ctypes
import ctypes.wintypes as w
import struct
import sys

a32 = ctypes.WinDLL("advapi32", use_last_error=True)
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.OpenProcess.restype = w.HANDLE
a32.ConvertSidToStringSidW.argtypes = [ctypes.c_void_p, ctypes.POINTER(w.LPWSTR)]
a32.GetTokenInformation.argtypes = [w.HANDLE, ctypes.c_int, ctypes.c_void_p, w.DWORD,
                                    ctypes.POINTER(w.DWORD)]
a32.LookupAccountSidW.argtypes = [w.LPCWSTR, ctypes.c_void_p, w.LPWSTR, ctypes.POINTER(w.DWORD),
                                  w.LPWSTR, ctypes.POINTER(w.DWORD), ctypes.POINTER(w.DWORD)]


def sid_text(address):
    out = w.LPWSTR()
    a32.ConvertSidToStringSidW(address, ctypes.byref(out))
    text = out.value
    name = ctypes.create_unicode_buffer(256)
    domain = ctypes.create_unicode_buffer(256)
    n, d, use = w.DWORD(256), w.DWORD(256), w.DWORD()
    if a32.LookupAccountSidW(None, address, name, ctypes.byref(n), domain, ctypes.byref(d),
                             ctypes.byref(use)):
        # The user's own name is private: only say it is the user.
        label = "(the user)" if use.value == 1 else f"({domain.value}\\{name.value})"
    else:
        label = ""
    return f"{text} {label}"


def info(token, cls):
    size = w.DWORD()
    a32.GetTokenInformation(token, cls, None, 0, ctypes.byref(size))
    buffer = ctypes.create_string_buffer(size.value or 8)
    if not a32.GetTokenInformation(token, cls, buffer, size, ctypes.byref(size)):
        return None, ctypes.get_last_error()
    return buffer, 0


def groups(token, cls, title):
    buffer, error = info(token, cls)
    if buffer is None:
        print(f"{title}: error {error}")
        return
    base = ctypes.addressof(buffer)
    count = struct.unpack_from("<I", buffer, 0)[0]
    print(f"{title}: {count}")
    for i in range(count):
        sid, attributes = struct.unpack_from("<QI", buffer, 8 + i * 16)
        flags = []
        if attributes & 0x10:
            flags.append("deny-only")
        if attributes & 0x20:
            flags.append("integrity")
        if attributes & 0xC0000000 == 0xC0000000:
            flags.append("logon")
        print(f"  {sid_text(sid)} {' '.join(flags)}")


def dacl(token):
    buffer, error = info(token, 6)  # TokenDefaultDacl
    if buffer is None:
        print(f"default DACL: error {error}")
        return
    acl = struct.unpack_from("<Q", buffer, 0)[0]
    raw = ctypes.string_at(acl, 8)
    _rev, _pad, size, count, _pad2 = struct.unpack("<BBHHH", raw)
    body = ctypes.string_at(acl, size)
    offset = 8
    print(f"default DACL: {count} entries")
    for _ in range(count):
        kind, flags, ace_size, mask = struct.unpack_from("<BBHI", body, offset)
        sid = acl + offset + 8
        print(f"  {'allow' if kind == 0 else kind} {mask:08X} {sid_text(sid)}")
        offset += ace_size


def main():
    pid = int(sys.argv[1])
    process = k32.OpenProcess(0x1000, False, pid)
    token = w.HANDLE()
    if not a32.OpenProcessToken(process, 0x0008, ctypes.byref(token)):
        print(f"OpenProcessToken: error {ctypes.get_last_error()}")
        return
    print(f"restricted: {bool(a32.IsTokenRestricted(token))}")
    groups(token, 2, "groups")
    groups(token, 11, "restricting SIDs")
    buffer, _ = info(token, 31)
    if buffer is not None:
        sid = struct.unpack_from("<Q", buffer, 0)[0]
        print(f"app container: {sid_text(sid) if sid else None}")
    dacl(token)


main()

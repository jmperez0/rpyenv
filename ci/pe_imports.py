"""CI guard (M6b design §7.1): Windows binaries link the C runtime statically.

Lists the DLLs a PE file imports. With --check, fails when any is the Visual C++
runtime (vcruntime*, msvcp*), which isn't on every machine (notably ARM64).
Usage: pe_imports.py --self-test | --check FILE... | FILE...
"""
import struct
import sys
from pathlib import Path

VC_RUNTIME = ("vcruntime", "msvcp")


def imports(data: bytes) -> list:
    """The imported DLL names, lowercased, in import-table order."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    coff = pe + 4
    (nsections,) = struct.unpack_from("<H", data, coff + 2)
    (opt_size,) = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    (magic,) = struct.unpack_from("<H", data, opt)
    directories = opt + (112 if magic == 0x20B else 96)
    (import_rva,) = struct.unpack_from("<I", data, directories + 8)
    sections = []
    for i in range(nsections):
        vsize, vaddr, rawsize, rawptr = struct.unpack_from(
            "<IIII", data, opt + opt_size + 40 * i + 8
        )
        sections.append((vaddr, max(vsize, rawsize), rawptr))

    def offset(rva):
        for vaddr, size, rawptr in sections:
            if vaddr <= rva < vaddr + size:
                return rawptr + rva - vaddr
        raise ValueError(f"RVA {rva:#x} is outside every section")

    names = []
    if import_rva == 0:
        return names
    at = offset(import_rva)
    while True:
        descriptor = struct.unpack_from("<5I", data, at)
        if descriptor == (0, 0, 0, 0, 0):
            return names
        start = offset(descriptor[3])
        names.append(data[start:data.index(b"\0", start)].decode("ascii").lower())
        at += 20


def vc_runtime(names) -> list:
    return [n for n in names if n.startswith(VC_RUNTIME)]


def synthetic_pe(dlls) -> bytes:
    """A minimal PE32+ file whose import table names `dlls` (one section at RVA 0x1000)."""
    data = bytearray(0x400)
    struct.pack_into("<I", data, 0x3C, 0x40)
    data[0x40:0x44] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x44, 0x8664, 1)
    struct.pack_into("<H", data, 0x44 + 16, 240)
    opt = 0x58
    struct.pack_into("<H", data, opt, 0x20B)
    struct.pack_into("<II", data, opt + 112 + 8, 0x1000, 20 * (len(dlls) + 1))
    section = opt + 240
    data[section:section + 8] = b".idata\0\0"
    struct.pack_into("<IIII", data, section + 8, 0x200, 0x1000, 0x200, 0x200)
    for i, dll in enumerate(dlls):
        name_rva = 0x1100 + 0x20 * i
        struct.pack_into("<5I", data, 0x200 + 20 * i, 0, 0, 0, name_rva, 0)
        start = 0x200 + name_rva - 0x1000
        data[start:start + len(dll)] = dll.encode("ascii")
    return bytes(data)


def self_test() -> None:
    names = imports(synthetic_pe(["KERNEL32.dll", "VCRUNTIME140.dll"]))
    assert names == ["kernel32.dll", "vcruntime140.dll"], names
    assert vc_runtime(names) == ["vcruntime140.dll"], vc_runtime(names)
    assert vc_runtime(imports(synthetic_pe(["KERNEL32.dll"]))) == []


def main(argv) -> int:
    if argv == ["--self-test"]:
        self_test()
        print("self-test ok")
        return 0
    check = argv[:1] == ["--check"]
    files = argv[1:] if check else argv
    if not files:
        print(__doc__, file=sys.stderr)
        return 2
    failed = False
    for f in files:
        names = imports(Path(f).read_bytes())
        bad = vc_runtime(names)
        print(f"{f}: {', '.join(names)}")
        if check and bad:
            print(f"{f} needs the Visual C++ runtime: {', '.join(bad)}")
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

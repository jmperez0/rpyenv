"""Installs and uninstalls rpyenv's MSI and checks what each step leaves (M6b design §7.4).

CI only: it changes the current user's PATH, PowerShell profiles and installed
programs, so it refuses to run unless CI=true (GitHub Actions sets it).
Usage: python installer/test_msi.py <dist-dir> <next-dist-dir>
"""
import ctypes
import os
import shutil
import subprocess
import sys
import winreg
from ctypes import wintypes
from pathlib import Path

LINE = 'iex ((pyenv init - pwsh) -join "`n")'
USER_DIR = Path(os.environ["LOCALAPPDATA"]) / "Programs" / "rpyenv"
MACHINE_DIR = Path(os.environ["ProgramFiles"]) / "rpyenv"
ROOT = Path(os.environ["USERPROFILE"]) / ".pyenv" / "pyenv-win"
SHIMS = ROOT / "shims"
ACTIVE_SETUP = r"SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}"
MACHINE_ENV = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"


def documents() -> Path:
    class GUID(ctypes.Structure):
        _fields_ = [("a", wintypes.DWORD), ("b", wintypes.WORD), ("c", wintypes.WORD), ("d", ctypes.c_ubyte * 8)]

    folder = GUID(0xFDD39AD0, 0x238F, 0x46AF, (ctypes.c_ubyte * 8)(0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7))
    path = ctypes.c_wchar_p()
    if ctypes.windll.shell32.SHGetKnownFolderPath(ctypes.byref(folder), 0, None, ctypes.byref(path)) != 0:
        raise OSError("SHGetKnownFolderPath(Documents) failed")
    result = Path(path.value)
    ctypes.windll.ole32.CoTaskMemFree(path)
    return result


PROFILES = [documents() / sub / "Microsoft.PowerShell_profile.ps1" for sub in ("WindowsPowerShell", "PowerShell")]


LOGS = Path("msi-logs")
_log_count = 0


def msiexec(args: str, expect=(0, 3010)) -> int:
    global _log_count
    _log_count += 1
    LOGS.mkdir(exist_ok=True)
    log = (LOGS / f"{_log_count:02d}-msiexec.log").resolve()
    code = subprocess.run(f'msiexec {args} /qn /l*v "{log}"', timeout=900).returncode
    if code not in expect:
        raw = log.read_bytes() if log.exists() else b""
        text = raw.decode("utf-16", "replace") if raw[:2] == b"\xff\xfe" else raw.decode("utf-8", "replace")
        lines = text.splitlines()
        # The failing action is the first "Return value 3"; show what led to it.
        failed = next((i for i, line in enumerate(lines) if "Return value 3" in line), None)
        shown = lines[max(0, failed - 40):failed + 5] if failed is not None else lines[-80:]
        raise AssertionError(f"msiexec {args}: exit {code}, expected {expect}\n" + "\n".join(shown))
    return code


def reg(root, key: str, name: str):
    try:
        with winreg.OpenKey(root, key, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as k:
            return winreg.QueryValueEx(k, name)[0]
    except FileNotFoundError:
        return None


def set_user_path(value: str) -> None:
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_SET_VALUE) as k:
        winreg.SetValueEx(k, "Path", 0, winreg.REG_EXPAND_SZ, value)


def entries(path) -> list:
    return [e.rstrip("\\").lower() for e in (path or "").split(";") if e.strip()]


def user_path() -> list:
    return entries(reg(winreg.HKEY_CURRENT_USER, "Environment", "Path"))


def machine_path() -> list:
    return entries(reg(winreg.HKEY_LOCAL_MACHINE, MACHINE_ENV, "Path"))


def profile_has_line() -> bool:
    return any(LINE in p.read_text(encoding="utf-8", errors="replace") for p in PROFILES if p.exists())


def check(ok: bool, what: str) -> None:
    if not ok:
        raise AssertionError(what)


def low(p: Path) -> str:
    return str(p).lower()


def per_user(msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    check((USER_DIR / "bin" / "pyenv.exe").is_file(), "pyenv.exe in LocalAppData\\Programs")
    check((USER_DIR / "completions" / "pyenv.pwsh").is_file(), "completions installed")
    check(low(USER_DIR / "bin") in user_path(), f"bin on the user PATH: {user_path()}")
    check(user_path()[:1] == [low(SHIMS)], f"shims first on the user PATH: {user_path()}")
    check((ROOT / ".rpyenv-setup").is_file(), "setup ran (marker)")
    check(profile_has_line(), "the PowerShell line")
    check(reg(winreg.HKEY_CURRENT_USER, r"Software\rpyenv", "Installed") == 1, "HKCU marker")
    msiexec(f'/x "{msi}"')
    check(not (USER_DIR / "bin" / "pyenv.exe").exists(), "files removed")
    check(low(USER_DIR / "bin") not in user_path(), f"bin off the user PATH: {user_path()}")
    check(low(SHIMS) not in user_path(), f"shims off the user PATH (setup --undo): {user_path()}")
    check(not profile_has_line(), "the PowerShell line removed")
    check(not (ROOT / ".rpyenv-setup").exists(), "marker removed")
    check((ROOT / "versions").is_dir(), "PYENV_ROOT kept")
    check(reg(winreg.HKEY_CURRENT_USER, r"Software\rpyenv", "Installed") is None, "HKCU marker removed")


def live_rehash(msi: Path) -> None:
    msiexec(f'/i "{msi}" LIVEREHASH=1')
    check(reg(winreg.HKEY_CURRENT_USER, "Environment", "RPYENV_LIVE_REHASH") == "1", "RPYENV_LIVE_REHASH=1")
    msiexec(f'/x "{msi}"')
    check(reg(winreg.HKEY_CURRENT_USER, "Environment", "RPYENV_LIVE_REHASH") is None, "RPYENV_LIVE_REHASH removed")


def migrate(msi: Path) -> None:
    bin_dir = ROOT / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    for name in ("pyenv.ps1", "pyenv.bat", "pyenv"):
        (bin_dir / name).write_text(f"{name} from pyenv-win\n", encoding="utf-8")
    before = reg(winreg.HKEY_CURRENT_USER, "Environment", "Path") or ""
    set_user_path(f"{bin_dir};{before}")
    try:
        msiexec(f'/i "{msi}" MIGRATEPYENVWIN=1')
        check((ROOT / ".rpyenv-migrate" / "bin" / "pyenv.ps1").is_file(), "pyenv-win's launchers backed up")
        check(not (bin_dir / "pyenv.ps1").exists(), "pyenv-win's launchers moved aside")
        check(low(bin_dir) not in user_path(), "pyenv-win's bin off the user PATH")
        msiexec(f'/x "{msi}"')
        check((bin_dir / "pyenv.ps1").is_file(), "pyenv-win's launchers restored")
        check(low(bin_dir) in user_path(), "pyenv-win's bin back on the user PATH")
        check(not (ROOT / ".rpyenv-migrate").exists(), "the migrate backup is gone")
    finally:
        set_user_path(before)
        for name in ("pyenv.ps1", "pyenv.bat", "pyenv"):
            (bin_dir / name).unlink(missing_ok=True)


def upgrade(msi: Path, next_msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    msiexec(f'/i "{next_msi}"')
    check(user_path().count(low(SHIMS)) == 1 and user_path()[:1] == [low(SHIMS)], f"shims still first, once: {user_path()}")
    check(user_path().count(low(USER_DIR / "bin")) == 1, f"bin once on the user PATH: {user_path()}")
    check((ROOT / ".rpyenv-setup").is_file(), "the upgrade didn't undo setup (marker)")
    check(profile_has_line(), "the upgrade didn't undo setup (profile line)")
    msiexec(f'/i "{msi}"', expect=(1603, 1638))  # a downgrade is refused
    msiexec(f'/x "{next_msi}"')
    check(not (USER_DIR / "bin" / "pyenv.exe").exists(), "the upgraded install uninstalls")


def scope_refusal(msi: Path, next_msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    # The same package again: Windows Installer sees it installed for this user (the same
    # product code) and runs maintenance; nothing goes to Program Files.
    msiexec(f'/i "{msi}" ALLUSERS=1 MSIINSTALLPERUSER=""')
    check(not (MACHINE_DIR / "bin").exists(), "the same package isn't installed for all users too")
    # A newer package can't see the per-user install from the machine context; the
    # scope marker refuses it (design §4.5).
    msiexec(f'/i "{next_msi}" ALLUSERS=1 MSIINSTALLPERUSER=""', expect=(1603,))
    check(not (MACHINE_DIR / "bin").exists(), "no all-users install over a per-user one")
    msiexec(f'/x "{msi}"')


def all_users(msi: Path, next_msi: Path) -> None:
    msiexec(f'/i "{msi}" ALLUSERS=1 MSIINSTALLPERUSER=""')
    check((MACHINE_DIR / "bin" / "pyenv.exe").is_file(), "pyenv.exe in Program Files")
    check(low(MACHINE_DIR / "bin") in machine_path(), f"bin on the machine PATH: {machine_path()}")
    stub = reg(winreg.HKEY_LOCAL_MACHINE, ACTIVE_SETUP, "StubPath") or ""
    check(stub.lower().endswith('pyenv.exe" setup'), f"Active Setup StubPath: {stub!r}")
    check(reg(winreg.HKEY_LOCAL_MACHINE, r"Software\rpyenv", "Installed") == 1, "HKLM marker")
    # The other direction (design §4.5): a newer just-for-me install over this one.
    msiexec(f'/i "{next_msi}"', expect=(1603,))
    check(not (USER_DIR / "bin" / "pyenv.exe").exists(), "no per-user install over an all-users one")
    msiexec(f'/x "{msi}" ALLUSERS=1')
    check(not (MACHINE_DIR / "bin" / "pyenv.exe").exists(), "files removed")
    check(low(MACHINE_DIR / "bin") not in machine_path(), "bin off the machine PATH")
    check(reg(winreg.HKEY_LOCAL_MACHINE, ACTIVE_SETUP, "StubPath") is None, "Active Setup key removed")


PUBLIC = Path(os.environ.get("PUBLIC", r"C:\Users\Public"))


def as_user(name: str, password: str, args: str) -> int:
    """Runs `msiexec <args> /qn` as local user `name`, with its profile loaded; its exit code.

    The log goes to Public (a standard user may not write the workspace) and is copied
    into msi-logs afterwards.
    """
    global _log_count
    _log_count += 1
    log = PUBLIC / f"rpyenv-{name}-{_log_count:02d}.log"
    # pwsh, not Windows PowerShell: started from a PowerShell 7 step, `powershell` inherits
    # its module path and can't load ConvertTo-SecureString. WaitForExit waits for msiexec
    # alone (Start-Process -Wait also waits for every descendant); reading Handle first keeps
    # the exit code readable.
    script = (
        "$ErrorActionPreference = 'Stop'; "
        f"$c = New-Object System.Management.Automation.PSCredential('{name}', "
        f"(ConvertTo-SecureString '{password}' -AsPlainText -Force)); "
        f"$p = Start-Process msiexec -ArgumentList '{args} /qn /l*v \"{log}\"' -Credential $c "
        "-LoadUserProfile -WorkingDirectory C:\\ -PassThru; $null = $p.Handle; "
        "$p.WaitForExit(); exit $p.ExitCode"
    )
    code = subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", script], timeout=900
    ).returncode
    if log.exists():
        LOGS.mkdir(exist_ok=True)
        shutil.copy(log, LOGS / log.name)
    return code


INSTALLER_POLICY = r"SOFTWARE\Policies\Microsoft\Windows\Installer"


def set_disable_msi(value):
    """Sets (or with None, deletes) the machine's DisableMSI policy; returns the old value."""
    old = reg(winreg.HKEY_LOCAL_MACHINE, INSTALLER_POLICY, "DisableMSI")
    with winreg.CreateKeyEx(winreg.HKEY_LOCAL_MACHINE, INSTALLER_POLICY, 0,
                            winreg.KEY_SET_VALUE | winreg.KEY_WOW64_64KEY) as k:
        if value is None:
            try:
                winreg.DeleteValue(k, "DisableMSI")
            except FileNotFoundError:
                pass
        else:
            winreg.SetValueEx(k, "DisableMSI", 0, winreg.REG_DWORD, value)
    return old


def standard_user(msi: Path) -> None:
    """Side-agent note (2026-10-10): "Just for me" must need no administrator rights.

    Installs and uninstalls as a fresh standard account. With /qn there is no prompt, so a
    step that needs elevation fails instead of asking.
    """
    # Windows Server turns Windows Installer off for non-administrators (machine policy
    # DisableMsi is 1 on the windows-2025 runner; error 1625). Client Windows, what "just for
    # me" is for, allows it: behave like it for this scenario, then put the policy back.
    old_policy = set_disable_msi(0)
    try:
        standard_user_install(msi)
    finally:
        set_disable_msi(old_policy)


def standard_user_install(msi: Path) -> None:
    import secrets

    name = "rpyenvstd"
    # 14 characters at most: `net user` asks "continue (Y/N)?" for a longer password, and
    # fails with nobody to answer. Upper, lower, digit and symbol meet the complexity rules.
    password = secrets.token_hex(5) + "Aa1-"
    added = subprocess.run(["net", "user", name, password, "/add"], capture_output=True, text=True)
    check(added.returncode == 0, f"net user /add: {added.returncode}\n{added.stdout}{added.stderr}")
    try:
        members = subprocess.run(["net", "localgroup", "Administrators"], capture_output=True, text=True).stdout
        check(name not in members, f"{name} is a standard account, not an administrator")
        home = Path(os.environ["SystemDrive"] + "\\") / "Users" / name
        # Public is readable by every user; the workspace may not be.
        shared = PUBLIC / msi.name
        shutil.copy(msi, shared)
        msi = shared
        code = as_user(name, password, f'/i "{msi}"')
        check(code in (0, 3010), f"just for me as a standard user: msiexec exit {code}")
        check((home / "AppData" / "Local" / "Programs" / "rpyenv" / "bin" / "pyenv.exe").is_file(),
              "pyenv.exe in the standard user's LocalAppData\\Programs")
        check((home / ".pyenv" / "pyenv-win" / ".rpyenv-setup").is_file(), "setup ran for the standard user")
        code = as_user(name, password, f'/x "{msi}"')
        check(code in (0, 3010), f"uninstall as a standard user: msiexec exit {code}")
        check(not (home / "AppData" / "Local" / "Programs" / "rpyenv" / "bin" / "pyenv.exe").exists(),
              "uninstalled for the standard user")
    finally:
        subprocess.run(["net", "user", name, "/delete"], capture_output=True)


def missing_exe(msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    (USER_DIR / "bin" / "pyenv.exe").unlink()
    msiexec(f'/x "{msi}"')  # the custom actions fail; the uninstall must not
    check(not (USER_DIR / "bin" / "pyenv-shim.exe").exists(), "uninstalled without pyenv.exe")


def only_msi(folder: str) -> Path:
    found = sorted(Path(folder).glob("*.msi"))
    if len(found) != 1:
        raise SystemExit(f"expected one .msi in {folder}, found {[f.name for f in found]}")
    return found[0].resolve()


def main(argv) -> int:
    if os.environ.get("CI") != "true":
        print("refusing: this changes the current user's PATH, profiles and installed programs; it runs on CI only")
        return 2
    if len(argv) != 2:
        print(__doc__)
        return 2
    msi, next_msi = only_msi(argv[0]), only_msi(argv[1])
    for name, run in [
        ("per_user", lambda: per_user(msi)),
        ("live_rehash", lambda: live_rehash(msi)),
        ("migrate", lambda: migrate(msi)),
        ("upgrade", lambda: upgrade(msi, next_msi)),
        ("scope_refusal", lambda: scope_refusal(msi, next_msi)),
        ("all_users", lambda: all_users(msi, next_msi)),
        ("standard_user", lambda: standard_user(msi)),
        ("missing_exe", lambda: missing_exe(msi)),  # last: it leaves setup's changes behind
    ]:
        run()
        print(f"ok {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

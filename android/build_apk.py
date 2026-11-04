#!/usr/bin/env python3
"""Builds the Android app: the Rust library for each phone architecture, packed into a signed APK.

    python android/build_apk.py                 build target/android/inkwell.apk (64-bit ARM, optimized)
    python android/build_apk.py --install       ... and install it on the connected device and start it
    python android/build_apk.py --device HOST   ... on this device when there are several (see `adb devices`)
    python android/build_apk.py --fast          an optimized build without link-time optimization (quicker to compile)
    python android/build_apk.py --debug         an unoptimized build (very slow to draw with)
    python android/build_apk.py --abi arm64-v8a --abi x86_64     several architectures

It needs the Android SDK (ANDROID_HOME, with an NDK, build-tools and a platform), `cargo-ndk`
(`cargo install cargo-ndk`) and a JDK for `keytool` and `apksigner`.

The APK is signed with the debug key of the machine (made if there is none): good for trying the app, and for nothing
else, since every machine has a key of its own and an APK signed with another one cannot be installed over it. For
releases, sign with a key that is kept, by naming it in the environment:

    INKWELL_KEYSTORE        the keystore file
    INKWELL_KEYSTORE_PASS   its password
    INKWELL_KEY_ALIAS       the key in it
    INKWELL_KEY_PASS        the key's password (the keystore's when left out)

The release workflow reads them from the repository secrets ANDROID_KEYSTORE_BASE64 (the keystore file, base64),
ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS and ANDROID_KEY_PASSWORD. A key is made with
`keytool -genkeypair -keystore inkwell.keystore -alias inkwell -keyalg RSA -keysize 2048 -validity 10000`.
"""

import argparse
import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ANDROID = ROOT / "android"
OUT = ROOT / "target" / "android"
PACKAGE = "app.inkwell"
ACTIVITY = "app.inkwell.InkActivity"
LIBRARY = "libink_android.so"
PLATFORM = "android-35"


def run(command, **kwargs):
    printable = " ".join(str(part) for part in command)
    print("+", printable if len(printable) < 200 else printable[:200] + " ...", flush=True)
    subprocess.run([str(part) for part in command], check=True, **kwargs)


def newest(folder):
    """The newest version folder inside `folder` (they are named by version)."""
    versions = sorted(p for p in folder.iterdir() if p.is_dir())
    if not versions:
        sys.exit(f"nothing installed in {folder}")
    return versions[-1]


def sdk():
    home = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if not home:
        for guess in (Path("C:/Android/Sdk"), Path.home() / "Android" / "Sdk", Path.home() / "Library/Android/sdk"):
            if guess.is_dir():
                home = str(guess)
                break
    if not home:
        sys.exit("set ANDROID_HOME to the Android SDK folder")
    return Path(home)


def tool(folder, name):
    """An executable of the SDK, with the extension this system gives it."""
    for extension in ("", ".exe", ".bat"):
        path = folder / (name + extension)
        if path.is_file():
            return path
    sys.exit(f"{name} not found in {folder}")


def compile_java(tools, android_jar):
    """Compiles the Java of the activity to `classes.dex` and says where it is."""
    sources = sorted((ANDROID / "java").rglob("*.java"))
    classes, dex = OUT / "classes", OUT / "dex"
    for folder in (classes, dex):
        if folder.exists():
            shutil.rmtree(folder)
        folder.mkdir(parents=True)
    run(["javac", "--release", "8", "-Xlint:-options", "-classpath", android_jar, "-d", classes, *sources])
    files = sorted(classes.rglob("*.class"))
    run([tool(tools, "d8"), "--min-api", "26", "--lib", android_jar, "--output", dex, *files])
    return dex / "classes.dex"


def debug_keystore():
    path = Path.home() / ".android" / "debug.keystore"
    if not path.is_file():
        path.parent.mkdir(parents=True, exist_ok=True)
        run([
            "keytool", "-genkeypair", "-v", "-keystore", path, "-storepass", "android", "-alias", "androiddebugkey",
            "-keypass", "android", "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
            "-dname", "CN=Android Debug,O=Android,C=US",
        ])
    return path


def signing():
    """The keystore, key and passwords to sign with: the ones in the environment, or else the debug key."""
    named = os.environ.get("INKWELL_KEYSTORE")
    if not named:
        return debug_keystore(), "androiddebugkey", "android", "android"
    keystore = Path(named)
    if not keystore.is_file():
        sys.exit(f"INKWELL_KEYSTORE names {keystore}, which is not a file")
    alias = os.environ.get("INKWELL_KEY_ALIAS")
    store_password = os.environ.get("INKWELL_KEYSTORE_PASS")
    if not alias or not store_password:
        sys.exit("INKWELL_KEYSTORE needs INKWELL_KEY_ALIAS and INKWELL_KEYSTORE_PASS with it")
    return keystore, alias, store_password, os.environ.get("INKWELL_KEY_PASS") or store_password


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--abi", action="append", help="architecture to build (default arm64-v8a)")
    parser.add_argument("--debug", action="store_true", help="an unoptimized build")
    parser.add_argument("--fast", action="store_true", help="optimized, without link-time optimization")
    parser.add_argument("--install", action="store_true", help="install on a device and start the app")
    parser.add_argument("--device", help="the device to install on (adb's name for it)")
    parser.add_argument("--no-build", action="store_true", help="pack the library already built")
    args = parser.parse_args()
    abis = args.abi or ["arm64-v8a"]

    home = sdk()
    env = dict(os.environ, ANDROID_HOME=str(home))
    ndk = newest(home / "ndk")
    env["ANDROID_NDK_HOME"] = str(ndk)
    tools = newest(home / "build-tools")
    android_jar = home / "platforms" / PLATFORM / "android.jar"
    if not android_jar.is_file():
        sys.exit(f"install the platform {PLATFORM} (sdkmanager \"platforms;{PLATFORM}\")")

    libs = ROOT / "target" / "android-libs"
    if not args.no_build:
        command = ["cargo", "ndk", *sum((["-t", abi] for abi in abis), []), "-o", libs, "build", "-p", "ink-android",
                   "--target-dir", OUT / "cargo"]
        if args.fast:
            command += ["--profile", "fast"]
        elif not args.debug:
            command.append("--release")
        run(command, cwd=ROOT, env=env)

    OUT.mkdir(parents=True, exist_ok=True)
    compiled, base, unaligned, apk = OUT / "res.zip", OUT / "base.apk", OUT / "unaligned.apk", OUT / "inkwell.apk"
    dex = compile_java(tools, android_jar)
    run([tool(tools, "aapt2"), "compile", "--dir", ANDROID / "res", "-o", compiled])
    run([
        tool(tools, "aapt2"), "link", "-o", base, "-I", android_jar, "--manifest", ANDROID / "AndroidManifest.xml",
        "-R", compiled, "--auto-add-overlay",
    ])
    with zipfile.ZipFile(base) as source, zipfile.ZipFile(unaligned, "w", zipfile.ZIP_DEFLATED) as target:
        for item in source.infolist():
            data = source.read(item.filename)
            # The resource table has to stay stored, not compressed, for the newest Android versions.
            kind = zipfile.ZIP_STORED if item.filename == "resources.arsc" else zipfile.ZIP_DEFLATED
            target.writestr(zipfile.ZipInfo(item.filename, item.date_time), data, kind)
        target.writestr(zipfile.ZipInfo("classes.dex", (2026, 1, 1, 0, 0, 0)), dex.read_bytes(), zipfile.ZIP_DEFLATED)
        for abi in abis:
            library = libs / abi / LIBRARY
            if not library.is_file():
                sys.exit(f"{library} was not built")
            info = zipfile.ZipInfo(f"lib/{abi}/{LIBRARY}", (2026, 1, 1, 0, 0, 0))
            target.writestr(info, library.read_bytes(), zipfile.ZIP_DEFLATED)
    run([tool(tools, "zipalign"), "-f", "-p", "4", unaligned, apk])
    keystore, alias, store_password, key_password = signing()
    run([
        tool(tools, "apksigner"), "sign", "--ks", keystore, "--ks-pass", f"pass:{store_password}",
        "--key-pass", f"pass:{key_password}", "--ks-key-alias", alias, apk,
    ])
    print(f"\nbuilt {apk} ({apk.stat().st_size / 1_048_576:.1f} MB)")

    if args.install:
        adb = [tool(home / "platform-tools", "adb")]
        if args.device:
            adb += ["-s", args.device]
        run(adb + ["install", "-r", apk])
        run(adb + ["shell", "am", "start", "-n", f"{PACKAGE}/{ACTIVITY}"])
        print("started; read what it says with: adb logcat -s inkwell RustStdoutStderr")


if __name__ == "__main__":
    main()

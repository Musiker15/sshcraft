//! `cargo xtask android [--check] [--debug] [--install]`: the APK for Android tablets, built from
//! `apps/android` with the NDK and put together with the SDK's own tools (aapt2, d8, zipalign,
//! apksigner) and the JDK's javac for the few Java classes in `apps/android/java`, no Gradle. `docs/android.md` says what to install.
//!
//! - `--check`: only clippy for the Android target on the crates that differ there
//! - `--debug`: a debug build (faster to build, slower to run)
//! - `--install`: `adb install -r` on the connected device afterwards
//!
//! Signing: `SSHCRAFT_ANDROID_KEYSTORE` names the release keystore (key alias `sshcraft`) and
//! `SSHCRAFT_ANDROID_KEYSTORE_PASS_FILE` a file holding its password. Without them the APK is
//! signed with a debug key made in `target/debug.keystore`, which says so; Android then refuses to
//! update an installed release with it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The app id on every platform (keyring service, data folder, desktop entry).
pub const PACKAGE: &str = "de.musiker15.sshcraft";
/// The library android-activity loads: `lib<LIB>.so`.
const LIB: &str = "sshcraft_android";
/// A CPU the APK is built for: the Rust target, Android's ABI name and the APK's suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub triple: &'static str,
    pub abi: &'static str,
    pub suffix: &'static str,
}

/// Tablets: the release APK.
pub const ARM64: Target = Target { triple: "aarch64-linux-android", abi: "arm64-v8a", suffix: "arm64" };
/// The x86_64 emulator on a desktop, for trying a build there (`--emulator`); never released.
pub const X86_64: Target = Target { triple: "x86_64-linux-android", abi: "x86_64", suffix: "x86_64" };
/// Android 13, decided for M1 (docs/android.md).
pub const MIN_SDK: u32 = 33;
pub const TARGET_SDK: u32 = 36;
const KEY_ALIAS: &str = "sshcraft";
/// Launcher icons from `assets/app-icon/hicolor`, one per density bucket.
const ICONS: [(&str, &str); 5] = [("mdpi", "48x48"), ("hdpi", "64x64"), ("xhdpi", "128x128"), ("xxhdpi", "256x256"), ("xxxhdpi", "512x512")];

const USAGE: &str = "usage: cargo xtask android [--check] [--debug] [--emulator] [--install]";

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let (mut check, mut debug, mut install) = (false, false, false);
    let mut target = ARM64;
    for arg in args {
        match *arg {
            "--check" => check = true,
            "--debug" => debug = true,
            "--emulator" => target = X86_64,
            "--install" => install = true,
            other => return Err(format!("android: unknown option `{other}`\n{USAGE}")),
        }
    }
    let sdk = Sdk::find()?;
    if check {
        return cargo(
            root,
            &sdk,
            target,
            &[
                "clippy",
                "--target",
                target.triple,
                "-p",
                "sshcraft-android",
                "-p",
                "sshcraft-ui-egui",
                "-p",
                "sshcraft-store",
                "--",
                "-D",
                "warnings",
            ],
        );
    }
    let mut build = vec!["build", "--target", target.triple, "-p", "sshcraft-android"];
    // The emulator's Vulkan driver crashes under wgpu; it draws with OpenGL ES.
    if target == X86_64 {
        build.extend(["--features", "sshcraft-android/gl"]);
    }
    if !debug {
        build.push("--release");
    }
    cargo(root, &sdk, target, &build)?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let version = crate::version::read(&manifest)?;
    let target_dir = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    let library = target_dir.join(target.triple).join(if debug { "debug" } else { "release" }).join(format!("lib{LIB}.so"));
    let work = target_dir.join("android");
    let apk = package(root, &sdk, target, &work, &library, &version)?;
    eprintln!("android: {}", apk.display());
    if install {
        let mut adb = Command::new(sdk.home.join("platform-tools").join(exe("adb")));
        adb.arg("install").arg("-r").arg(&apk);
        crate::run(adb, "adb install")?;
    }
    Ok(())
}

/// `.exe` on Windows.
fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// The SDK and NDK found on this machine.
struct Sdk {
    home: PathBuf,
    ndk: PathBuf,
    build_tools: PathBuf,
    platform_jar: PathBuf,
}

impl Sdk {
    fn find() -> Result<Self, String> {
        let home = std::env::var_os("ANDROID_HOME")
            .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
            .map(PathBuf::from)
            .ok_or("android: set ANDROID_HOME to the Android SDK (docs/android.md)")?;
        let ndk = match std::env::var_os("NDK_HOME").or_else(|| std::env::var_os("ANDROID_NDK_HOME")) {
            Some(ndk) => PathBuf::from(ndk),
            None => home.join("ndk").join(newest(&dir_names(&home.join("ndk")), 0).ok_or("android: no NDK under ANDROID_HOME/ndk")?),
        };
        let build_tools =
            home.join("build-tools").join(newest(&dir_names(&home.join("build-tools")), 0).ok_or("android: no build-tools in the SDK")?);
        let platforms = dir_names(&home.join("platforms"));
        let numbers: Vec<String> = platforms.iter().filter_map(|p| p.strip_prefix("android-")).map(str::to_string).collect();
        let platform = newest(&numbers, TARGET_SDK).ok_or(format!("android: no platform android-{TARGET_SDK} or newer in the SDK"))?;
        let platform_jar = home.join("platforms").join(format!("android-{platform}")).join("android.jar");
        Ok(Self { home, ndk, build_tools, platform_jar })
    }

    /// The NDK's clang for the minimum API level, the linker and C compiler of the target.
    fn clang(&self, target: Target) -> Result<PathBuf, String> {
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        let name = format!("{}{MIN_SDK}-clang{}", target.triple, if cfg!(windows) { ".cmd" } else { "" });
        let clang = self.ndk.join("toolchains").join("llvm").join("prebuilt").join(host).join("bin").join(name);
        if clang.is_file() { Ok(clang) } else { Err(format!("android: {} is missing", clang.display())) }
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(exe(name))
    }

    /// `java -jar <build-tools>/lib/apksigner.jar`: the jar, not the shell script, so no shell
    /// reads its arguments.
    fn apksigner(&self) -> Command {
        let java = std::env::var_os("JAVA_HOME").map_or_else(|| PathBuf::from("java"), |home| PathBuf::from(home).join("bin").join(exe("java")));
        let mut cmd = Command::new(java);
        cmd.arg("-jar").arg(self.build_tools.join("lib").join("apksigner.jar"));
        cmd
    }
}

/// The names of the folders in `dir`; none when it cannot be read.
fn dir_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// The highest version among `names` (dotted numbers, compared number by number) whose first
/// number is at least `at_least`. Names that are not versions are left out.
pub fn newest(names: &[String], at_least: u32) -> Option<String> {
    let parse = |name: &str| -> Option<Vec<u32>> { name.split('.').map(|part| part.parse().ok()).collect() };
    names
        .iter()
        .filter_map(|name| parse(name).map(|v| (v, name)))
        .filter(|(v, _)| v.first().is_some_and(|first| *first >= at_least))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, name)| name.clone())
}

/// Android's versionCode for `version`: major * 10000 + minor * 100 + patch, so every release
/// is higher than the one before. A pre-release suffix (`-rc.1`) counts as the release.
pub fn version_code(version: &str) -> Result<u32, String> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<u32> =
        core.split('.').map(|p| p.parse().map_err(|_| format!("android: `{version}` is not a version"))).collect::<Result<_, _>>()?;
    let [major, minor, patch] = parts[..] else { return Err(format!("android: `{version}` is not major.minor.patch")) };
    if minor > 99 || patch > 99 || major > 20_000 {
        return Err(format!("android: `{version}` does not fit a versionCode"));
    }
    Ok(major * 10_000 + minor * 100 + patch)
}

/// The manifest: SSHCraft's NativeActivity (`MainActivity`, with the keyboard's text
/// connection) that loads `lib<LIB>.so`, kept alive across every change it
/// can follow (a restart would end the process, see `apps/android`), the window resized for the
/// on-screen keyboard, network access, and no backup or device transfer of the app's data: the
/// vault's key stays in this device's Keystore, so copied data could not be opened.
pub fn manifest(version: &str, code: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{PACKAGE}"
    android:versionCode="{code}"
    android:versionName="{version}">
    <uses-sdk android:minSdkVersion="{MIN_SDK}" android:targetSdkVersion="{TARGET_SDK}" />
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <application
        android:label="SSHCraft"
        android:icon="@mipmap/ic_launcher"
        android:hasCode="true"
        android:allowBackup="false"
        android:dataExtractionRules="@xml/data_extraction_rules"
        android:extractNativeLibs="false">
        <activity
            android:name="de.musiker15.sshcraft.MainActivity"
            android:exported="true"
            android:launchMode="singleTask"
            android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|density|uiMode|locale|layoutDirection|fontScale|colorMode|touchscreen|mcc|mnc"
            android:windowSoftInputMode="stateHidden|adjustResize">
            <meta-data android:name="android.app.lib_name" android:value="{LIB}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    )
}

/// Android 12 and later: nothing goes into a cloud backup or a transfer to a new device.
pub const DATA_EXTRACTION_RULES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<data-extraction-rules>
    <cloud-backup>
        <exclude domain="root" path="." />
        <exclude domain="file" path="." />
        <exclude domain="database" path="." />
        <exclude domain="sharedpref" path="." />
        <exclude domain="external" path="." />
    </cloud-backup>
    <device-transfer>
        <exclude domain="root" path="." />
        <exclude domain="file" path="." />
        <exclude domain="database" path="." />
        <exclude domain="sharedpref" path="." />
        <exclude domain="external" path="." />
    </device-transfer>
</data-extraction-rules>
"#;

/// The path of the native library inside the APK.
pub fn library_entry(target: Target) -> String {
    format!("lib/{}/lib{LIB}.so", target.abi)
}

/// `cargo <args>` with the NDK as C compiler, archiver and linker of the target.
fn cargo(root: &Path, sdk: &Sdk, target: Target, args: &[&str]) -> Result<(), String> {
    let clang = sdk.clang(target)?;
    let ar = clang.with_file_name(exe("llvm-ar"));
    let key = target.triple.replace('-', "_");
    let mut cmd = crate::cargo();
    cmd.current_dir(root)
        .args(args)
        .env(format!("CC_{key}"), &clang)
        .env(format!("AR_{key}"), ar)
        .env(format!("CARGO_TARGET_{}_LINKER", key.to_uppercase()), &clang);
    crate::run(cmd, &format!("cargo {}", args.first().copied().unwrap_or_default()))
}

/// Links the resources, adds the library, aligns and signs; returns the APK in `dist/android`.
fn package(root: &Path, sdk: &Sdk, target: Target, work: &Path, library: &Path, version: &str) -> Result<PathBuf, String> {
    let io = |what: &str, e: std::io::Error| format!("android: {what}: {e}");
    if !library.is_file() {
        return Err(format!("android: {} was not built", library.display()));
    }
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| io("target/android", e))?;
    std::fs::write(work.join("AndroidManifest.xml"), manifest(version, version_code(version)?)).map_err(|e| io("the manifest", e))?;
    for (density, size) in ICONS {
        let from = root.join("assets/app-icon/hicolor").join(size).join("apps").join(format!("{PACKAGE}.png"));
        let dir = work.join("res").join(format!("mipmap-{density}"));
        std::fs::create_dir_all(&dir).map_err(|e| io("res", e))?;
        std::fs::copy(&from, dir.join("ic_launcher.png")).map_err(|e| io(&from.display().to_string(), e))?;
    }
    let xml = work.join("res").join("xml");
    std::fs::create_dir_all(&xml).map_err(|e| io("res/xml", e))?;
    std::fs::write(xml.join("data_extraction_rules.xml"), DATA_EXTRACTION_RULES).map_err(|e| io("the data extraction rules", e))?;
    let mut compile = Command::new(sdk.tool("aapt2"));
    compile.arg("compile").arg("--dir").arg(work.join("res")).arg("-o").arg(work.join("res.zip"));
    crate::run(compile, "aapt2 compile")?;
    let linked = work.join("linked.apk");
    let mut link = Command::new(sdk.tool("aapt2"));
    link.arg("link")
        .arg("-o")
        .arg(&linked)
        .arg("-I")
        .arg(&sdk.platform_jar)
        .arg("--manifest")
        .arg(work.join("AndroidManifest.xml"))
        .arg(work.join("res.zip"));
    crate::run(link, "aapt2 link")?;
    let dex = java(root, sdk, work)?;
    let unaligned = work.join("unaligned.apk");
    add_files(&linked, &[(library_entry(target), library.to_path_buf()), ("classes.dex".to_string(), dex)], &unaligned)?;
    let aligned = work.join("aligned.apk");
    let mut align = Command::new(sdk.tool("zipalign"));
    // 16 KiB pages for the uncompressed library (Android 15 devices may use them).
    align.args(["-P", "16", "-f", "4"]).arg(&unaligned).arg(&aligned);
    crate::run(align, "zipalign")?;
    let out_dir = root.join("dist").join("android");
    std::fs::create_dir_all(&out_dir).map_err(|e| io("dist/android", e))?;
    let apk = out_dir.join(format!("sshcraft-{version}-android-{}.apk", target.suffix));
    let mut sign = sdk.apksigner();
    sign.arg("sign");
    signing_key(work, &mut sign)?;
    sign.arg("--out").arg(&apk).arg(&aligned);
    crate::run(sign, "apksigner sign")?;
    let mut verify = sdk.apksigner();
    verify.arg("verify").arg(&apk);
    crate::run(verify, "apksigner verify")?;
    Ok(apk)
}

/// The keystore arguments for apksigner: the release key from the environment, or a debug key.
fn signing_key(work: &Path, sign: &mut Command) -> Result<(), String> {
    if let (Some(store), Some(pass)) = (std::env::var_os("SSHCRAFT_ANDROID_KEYSTORE"), std::env::var_os("SSHCRAFT_ANDROID_KEYSTORE_PASS_FILE")) {
        // apksigner reads the password from the file itself; it never shows up in a command line.
        let mut pass_arg = std::ffi::OsString::from("file:");
        pass_arg.push(pass);
        sign.arg("--ks").arg(store).arg("--ks-key-alias").arg(KEY_ALIAS).arg("--ks-pass").arg(pass_arg);
        return Ok(());
    }
    eprintln!("android: SSHCRAFT_ANDROID_KEYSTORE is not set; signing with a debug key (docs/android.md)");
    let store = work.parent().unwrap_or(work).join("debug.keystore");
    if !store.is_file() {
        let keytool =
            std::env::var_os("JAVA_HOME").map_or_else(|| PathBuf::from("keytool"), |home| PathBuf::from(home).join("bin").join(exe("keytool")));
        let mut make = Command::new(keytool);
        make.args(["-genkeypair", "-keystore"])
            .arg(&store)
            .args([
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
            ])
            .args(["-dname", "CN=SSHCraft Debug,O=SSHCraft,C=DE"]);
        crate::run(make, "keytool")?;
    }
    sign.arg("--ks").arg(&store).args(["--ks-key-alias", "androiddebugkey", "--ks-pass", "pass:android"]);
    Ok(())
}

/// The `.java` files under `dir`, sorted, at most a few folders deep.
pub fn java_sources(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() && depth < 8 {
                walk(&path, depth + 1, out);
            } else if path.extension().is_some_and(|e| e == "java") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out.sort();
    out
}

/// The JDK's tool `name` (javac, java, keytool): from `JAVA_HOME`, else from the `PATH`.
fn jdk(name: &str) -> PathBuf {
    std::env::var_os("JAVA_HOME").map_or_else(|| PathBuf::from(name), |home| PathBuf::from(home).join("bin").join(exe(name)))
}

/// Compiles `apps/android/java` against the platform's android.jar and turns it into
/// classes.dex with d8; returns that file.
fn java(root: &Path, sdk: &Sdk, work: &Path) -> Result<PathBuf, String> {
    let sources = java_sources(&root.join("apps/android/java"));
    if sources.is_empty() {
        return Err("android: no Java sources in apps/android/java".into());
    }
    let classes = work.join("classes");
    let mut javac = Command::new(jdk("javac"));
    javac
        .args(["-source", "17", "-target", "17", "-nowarn", "-encoding", "UTF-8", "-classpath"])
        .arg(&sdk.platform_jar)
        .arg("-d")
        .arg(&classes)
        .args(&sources);
    crate::run(javac, "javac")?;
    let mut compiled = Vec::new();
    let mut stack = vec![classes.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("android: {}: {e}", dir.display()))?.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "class") {
                compiled.push(path);
            }
        }
    }
    let dex_dir = work.join("dex");
    std::fs::create_dir_all(&dex_dir).map_err(|e| format!("android: {}: {e}", dex_dir.display()))?;
    let mut d8 = Command::new(jdk("java"));
    d8.arg("-cp")
        .arg(sdk.build_tools.join("lib").join("d8.jar"))
        .arg("com.android.tools.r8.D8")
        .args(["--release", "--min-api", &MIN_SDK.to_string(), "--lib"])
        .arg(&sdk.platform_jar)
        .arg("--output")
        .arg(&dex_dir)
        .args(&compiled);
    crate::run(d8, "d8")?;
    Ok(dex_dir.join("classes.dex"))
}

/// Copies `linked` to `out` and adds `files` (path in the APK, file) uncompressed: Android maps
/// the library straight from the APK.
fn add_files(linked: &Path, files: &[(String, PathBuf)], out: &Path) -> Result<(), String> {
    let zip_err = |e: zip::result::ZipError| format!("android: the APK: {e}");
    let io_err = |e: std::io::Error| format!("android: the APK: {e}");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(linked).map_err(io_err)?).map_err(zip_err)?;
    let mut writer = zip::ZipWriter::new(std::fs::File::create(out).map_err(io_err)?);
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(zip_err)?;
        writer.raw_copy_file(entry).map_err(zip_err)?;
    }
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (entry, file) in files {
        writer.start_file(entry.as_str(), options).map_err(zip_err)?;
        let bytes = std::fs::read(file).map_err(io_err)?;
        writer.write_all(&bytes).map_err(io_err)?;
    }
    writer.finish().map_err(zip_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_code_orders_releases() {
        assert_eq!(version_code("0.2.0"), Ok(200));
        assert_eq!(version_code("1.2.3"), Ok(10203));
        assert_eq!(version_code("0.3.0-rc.1"), Ok(300));
        assert!(version_code("0.3.0").unwrap() > version_code("0.2.99").unwrap());
        assert!(version_code("1.2").is_err());
        assert!(version_code("0.100.0").is_err(), "minor 100 would collide with major 1");
    }

    #[test]
    fn the_manifest_names_the_native_activity() {
        let m = manifest("0.3.0", 300);
        for wanted in [
            r#"package="de.musiker15.sshcraft""#,
            r#"android:versionCode="300""#,
            r#"android:versionName="0.3.0""#,
            r#"android:minSdkVersion="33""#,
            r#"android:targetSdkVersion="36""#,
            "android.permission.INTERNET",
            r#"android:name="de.musiker15.sshcraft.MainActivity""#,
            r#"android:hasCode="true""#,
            r#"android:value="sshcraft_android""#,
            r#"android:windowSoftInputMode="stateHidden|adjustResize""#,
            r#"android:extractNativeLibs="false""#,
            r#"android:allowBackup="false""#,
            "keyboardHidden",
            "fontScale",
            "locale",
            r#"android:dataExtractionRules="@xml/data_extraction_rules""#,
        ] {
            assert!(m.contains(wanted), "the manifest lacks {wanted}");
        }
        assert_eq!(library_entry(ARM64), "lib/arm64-v8a/libsshcraft_android.so");
        assert_eq!(library_entry(X86_64), "lib/x86_64/libsshcraft_android.so");
    }

    #[test]
    fn the_activity_class_is_built_from_the_tree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let sources = java_sources(&root.join("apps/android/java"));
        let names: Vec<String> = sources.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"MainActivity.java".to_string()), "found {names:?}");
        assert!(manifest("0.3.0", 300).contains("de.musiker15.sshcraft.MainActivity"));
    }

    #[test]
    fn no_data_leaves_the_device() {
        for part in ["<cloud-backup>", "<device-transfer>"] {
            let start = DATA_EXTRACTION_RULES.find(part).unwrap();
            let section = &DATA_EXTRACTION_RULES[start..];
            for domain in ["root", "file", "database", "sharedpref", "external"] {
                assert!(section.contains(&format!(r#"<exclude domain="{domain}" path="." />"#)), "{part} keeps {domain}");
            }
        }
    }

    #[test]
    fn the_newest_build_tools_are_picked() {
        let names: Vec<String> = ["36.0.0", "37.0.0", "9.0.0", "37.0.0-rc1", "notes"].iter().map(|s| s.to_string()).collect();
        assert_eq!(newest(&names, 0).as_deref(), Some("37.0.0"));
        let platforms: Vec<String> = ["30", "36", "35"].iter().map(|s| s.to_string()).collect();
        assert_eq!(newest(&platforms, 36).as_deref(), Some("36"));
        assert_eq!(newest(&platforms, 37), None);
    }
}

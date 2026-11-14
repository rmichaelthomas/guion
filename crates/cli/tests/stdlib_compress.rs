//! `std:compress` and `std:tar` (artifact format 71): gzip and zlib over bytes, streamed gzip
//! file variants, and tar write, read and list under the `std:fs` confinement rules.
//!
//! Fixtures that an outside implementation must be able to read, or that Guion must be able to
//! read, are produced by `python3` (`gzip`, `zlib`, `tarfile`) and by the system `tar`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("guion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root")).unwrap();
        Scratch(dir)
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(scratch: &Scratch, manifest: Option<&str>, source: &str) -> Output {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
    if let Some(manifest) = manifest {
        std::fs::write(scratch.0.join("guion.toml"), manifest).unwrap();
    }
    let root = scratch.root();
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--file-root", root.to_str().unwrap(), "main.gu"])
        .current_dir(&scratch.0)
        .output()
        .expect("guion starts")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn python(script: &str, args: &[&str]) {
    let output = Command::new("python3")
        .args(["-I", "-c", script])
        .args(args)
        .output()
        .expect("python3 starts");
    assert!(
        output.status.success(),
        "python3 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

const HEAD: &str = "import \"std:compress\"\nimport \"std:tar\"\nimport \"std:types\"\nimport \"std:encoding\"\nimport \"std:text\"\n\n";

fn program(body: &str) -> String {
    format!("{HEAD}fn main() -> Int:\n{body}    return 0\n")
}

/// A sample with structure, so deflate has something to find, and non-ASCII bytes.
fn sample(len: usize) -> Vec<u8> {
    let words = [
        "alpha",
        "beta",
        "gamma",
        "delta",
        "epsilon",
        "caf\u{e9}",
        "\u{1f600}",
    ];
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut out = Vec::with_capacity(len + 16);
    while out.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.extend_from_slice(words[(state % words.len() as u64) as usize].as_bytes());
        out.push(b' ');
    }
    out.truncate(len);
    out
}

#[test]
fn inflate_reads_bytes_made_by_python_gzip_and_zlib_byte_for_byte() {
    let scratch = Scratch::new("compress-python");
    let data = sample(300_000);
    std::fs::write(scratch.root().join("plain.bin"), &data).unwrap();
    let root = scratch.root();
    python(
        "import gzip, zlib, sys\nr = sys.argv[1]\nd = open(r + '/plain.bin', 'rb').read()\nopen(r + '/py.gz', 'wb').write(gzip.compress(d, mtime=0))\nopen(r + '/py.zlib', 'wb').write(zlib.compress(d, 9))\nopen(r + '/empty.gz', 'wb').write(gzip.compress(b'', mtime=0))\nopen(r + '/empty.zlib', 'wb').write(zlib.compress(b''))\n",
        &[root.to_str().unwrap()],
    );
    let source = program(
        r#"    let g = unwrap(read_bytes("py.gz"))
    let z = unwrap(read_bytes("py.zlib"))
    let plain = unwrap(read_bytes("plain.bin"))
    assert(unwrap(gzip_decompress(g, 10000000)) == plain)
    assert(unwrap(zlib_decompress(z, 10000000)) == plain)
    let w = unwrap(write_bytes("out-g.bin", unwrap(gzip_decompress(g, 10000000))))
    assert(len(bytes_to_ints(unwrap(gzip_decompress(unwrap(read_bytes("empty.gz")), 0)))) == 0)
    assert(len(bytes_to_ints(unwrap(zlib_decompress(unwrap(read_bytes("empty.zlib")), 0)))) == 0)
"#,
    );
    ok(&run(&scratch, None, &source));
    assert_eq!(
        std::fs::read(scratch.root().join("out-g.bin")).unwrap(),
        data
    );
}

#[test]
fn python_inflates_what_guion_deflates() {
    let scratch = Scratch::new("compress-to-python");
    let data = sample(200_000);
    std::fs::write(scratch.root().join("plain.bin"), &data).unwrap();
    let source = program(
        r#"    let plain = unwrap(read_bytes("plain.bin"))
    let a = unwrap(write_bytes("g.gz", gzip_compress(plain)))
    let b = unwrap(write_bytes("z.zlib", zlib_compress(plain)))
"#,
    );
    ok(&run(&scratch, None, &source));
    let root = scratch.root();
    python(
        "import gzip, zlib, sys\nr = sys.argv[1]\nd = open(r + '/plain.bin', 'rb').read()\nassert gzip.decompress(open(r + '/g.gz', 'rb').read()) == d\nassert zlib.decompress(open(r + '/z.zlib', 'rb').read()) == d\nassert len(open(r + '/g.gz', 'rb').read()) < len(d) // 2\n",
        &[root.to_str().unwrap()],
    );
}

#[test]
fn deflate_then_inflate_round_trips_including_empty_and_incompressible_input() {
    let scratch = Scratch::new("compress-roundtrip");
    let source = program(
        r#"    let samples = [bytes_from_text(""), bytes_from_text("a"), bytes_from_text("hello hello hello hello"), bytes_from_ints([0, 255, 1, 254, 2, 253, 3, 252])]
    for item in samples:
        assert(unwrap(gzip_decompress(gzip_compress(item), 1000)) == item)
        assert(unwrap(zlib_decompress(zlib_compress(item), 1000)) == item)
"#,
    );
    ok(&run(&scratch, None, &source));
}

#[test]
fn corrupt_and_truncated_input_are_typed_errors() {
    let scratch = Scratch::new("compress-corrupt");
    let data = sample(50_000);
    std::fs::write(scratch.root().join("plain.bin"), &data).unwrap();
    let root = scratch.root();
    python(
        "import gzip, zlib, sys\nr = sys.argv[1]\nd = open(r + '/plain.bin', 'rb').read()\ng = gzip.compress(d, mtime=0)\nz = zlib.compress(d)\nopen(r + '/trunc.gz', 'wb').write(g[:len(g)//2])\nopen(r + '/trunc.zlib', 'wb').write(z[:len(z)//2])\nbad = bytearray(g)\nbad[len(bad)//2] ^= 0xff\nopen(r + '/bad.gz', 'wb').write(bytes(bad))\nbadz = bytearray(z)\nbadz[len(badz)//2] ^= 0xff\nopen(r + '/bad.zlib', 'wb').write(bytes(badz))\nopen(r + '/notgz.bin', 'wb').write(b'this is not gzip at all')\nopen(r + '/crc.gz', 'wb').write(g[:-8] + bytes([g[-8] ^ 1]) + g[-7:])\n",
        &[root.to_str().unwrap()],
    );
    let source = program(
        r#"    let names = ["trunc.gz", "bad.gz", "notgz.bin", "crc.gz"]
    for name in names:
        match gzip_decompress(unwrap(read_bytes(name)), 10000000):
            Ok(data):
                print("unexpected ok")
            Err(problem):
                print(name + " " + problem.code)
    match zlib_decompress(unwrap(read_bytes("trunc.zlib")), 10000000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("trunc.zlib " + problem.code)
    match zlib_decompress(unwrap(read_bytes("bad.zlib")), 10000000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("bad.zlib " + problem.code)
    match gzip_decompress(bytes_from_text(""), 10):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("empty " + problem.code)
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains("trunc.gz E_COMPRESS_TRUNCATED"), "{out}");
    assert!(out.contains("bad.gz E_COMPRESS_CORRUPT"), "{out}");
    assert!(out.contains("notgz.bin E_COMPRESS_CORRUPT"), "{out}");
    assert!(out.contains("crc.gz E_COMPRESS_CORRUPT"), "{out}");
    assert!(out.contains("trunc.zlib E_COMPRESS_TRUNCATED"), "{out}");
    assert!(out.contains("bad.zlib E_COMPRESS_CORRUPT"), "{out}");
    assert!(out.contains("empty E_COMPRESS_TRUNCATED"), "{out}");
}

#[test]
fn a_zip_bomb_is_refused_at_the_ceiling_for_gzip_and_zlib() {
    let scratch = Scratch::new("compress-bomb");
    let root = scratch.root();
    // 256 MiB of zeros deflates to roughly 250 KB.
    python(
        "import gzip, zlib, sys\nr = sys.argv[1]\nd = bytes(256 * 1024 * 1024)\nopen(r + '/bomb.gz', 'wb').write(gzip.compress(d, 9, mtime=0))\nopen(r + '/bomb.zlib', 'wb').write(zlib.compress(d, 9))\nopen(r + '/ok.gz', 'wb').write(gzip.compress(bytes(1000), mtime=0))\n",
        &[root.to_str().unwrap()],
    );
    assert!(std::fs::metadata(root.join("bomb.gz")).unwrap().len() < 1_000_000);
    let source = program(
        r#"    match gzip_decompress(unwrap(read_bytes("bomb.gz")), 1000000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("gz " + problem.code)
    match zlib_decompress(unwrap(read_bytes("bomb.zlib")), 1000000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("zlib " + problem.code)
    # Exactly the ceiling is allowed, one byte under is refused.
    let small = unwrap(read_bytes("ok.gz"))
    assert(len(bytes_to_ints(unwrap(gzip_decompress(small, 1000)))) == 1000)
    match gzip_decompress(small, 999):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("under " + problem.code)
    match gzip_decompress(small, -1):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("negative " + problem.code)
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains("gz E_COMPRESS_LIMIT"), "{out}");
    assert!(out.contains("zlib E_COMPRESS_LIMIT"), "{out}");
    assert!(out.contains("under E_COMPRESS_LIMIT"), "{out}");
    assert!(out.contains("negative E_COMPRESS_ARGUMENT"), "{out}");
}

#[test]
fn the_runtime_ceiling_in_guion_toml_caps_an_explicit_max_output() {
    let scratch = Scratch::new("compress-policy-limit");
    let root = scratch.root();
    python(
        "import gzip, sys\nr = sys.argv[1]\nopen(r + '/k.gz', 'wb').write(gzip.compress(bytes(5000), mtime=0))\nopen(r + '/k.txt', 'wb').write(bytes(5000))\n",
        &[root.to_str().unwrap()],
    );
    let source = program(
        r#"    match gzip_decompress(unwrap(read_bytes("k.gz")), 1000000000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("bytes " + problem.code)
    match gzip_decompress_file("k.gz", "k.out"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("file " + problem.code)
"#,
    );
    let out = ok(&run(
        &scratch,
        Some("[runtime]\ndecompress_limit_bytes = 1000\n"),
        &source,
    ));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains("bytes E_COMPRESS_LIMIT"), "{out}");
    assert!(out.contains("file E_COMPRESS_LIMIT"), "{out}");
    // A refused file decompression leaves no partial destination behind.
    assert!(!scratch.root().join("k.out").exists());
    // The key is validated like the other [runtime] keys.
    let bad = run(
        &scratch,
        Some("[runtime]\ndecompress_limit_bytes = 0\n"),
        &program(""),
    );
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("decompress_limit_bytes"));
}

#[test]
fn trailing_garbage_is_an_error_and_multi_member_gzip_concatenates() {
    let scratch = Scratch::new("compress-members");
    let root = scratch.root();
    python(
        "import gzip, zlib, sys\nr = sys.argv[1]\na = gzip.compress(b'first,', mtime=0)\nb = gzip.compress(b'second,', mtime=0)\nc = gzip.compress(b'third', mtime=0)\nopen(r + '/multi.gz', 'wb').write(a + b + c)\nopen(r + '/garbage.gz', 'wb').write(a + b'garbage after the member')\nopen(r + '/zeros.gz', 'wb').write(a + bytes(16))\nopen(r + '/garbage2.gz', 'wb').write(a + b + b'xx')\nopen(r + '/garbage.zlib', 'wb').write(zlib.compress(b'zzz') + b'extra')\nopen(r + '/empty-member.gz', 'wb').write(gzip.compress(b'', mtime=0) + b)\n",
        &[root.to_str().unwrap()],
    );
    let source = program(
        r#"    let multi = unwrap(gzip_decompress(unwrap(read_bytes("multi.gz")), 1000))
    print(unwrap(encoding_utf8_decode(multi)))
    let mixed = unwrap(gzip_decompress(unwrap(read_bytes("empty-member.gz")), 1000))
    print(unwrap(encoding_utf8_decode(mixed)))
    for name in ["garbage.gz", "zeros.gz", "garbage2.gz"]:
        match gzip_decompress(unwrap(read_bytes(name)), 1000):
            Ok(data):
                print("unexpected ok")
            Err(problem):
                print(name + " " + problem.code)
    match zlib_decompress(unwrap(read_bytes("garbage.zlib")), 1000):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("zlib " + problem.code)
    # The limit counts all members together.
    match gzip_decompress(unwrap(read_bytes("multi.gz")), 15):
        Ok(data):
            print("unexpected ok")
        Err(problem):
            print("multi-limit " + problem.code)
    let exact = unwrap(gzip_decompress(unwrap(read_bytes("multi.gz")), 18))
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains("first,second,third"), "{out}");
    assert!(out.contains("second,"), "{out}");
    assert!(out.contains("garbage.gz E_COMPRESS_TRAILING_DATA"), "{out}");
    assert!(out.contains("zeros.gz E_COMPRESS_TRAILING_DATA"), "{out}");
    assert!(
        out.contains("garbage2.gz E_COMPRESS_TRAILING_DATA"),
        "{out}"
    );
    assert!(out.contains("zlib E_COMPRESS_TRAILING_DATA"), "{out}");
    assert!(out.contains("multi-limit E_COMPRESS_LIMIT"), "{out}");
}

#[test]
fn gzip_files_stream_both_ways_and_python_reads_the_result() {
    let scratch = Scratch::new("compress-files");
    let root = scratch.root();
    let data = sample(2_000_000);
    std::fs::write(root.join("plain.txt"), &data).unwrap();
    let source = program(
        r#"    let packed = unwrap(gzip_compress_file("plain.txt", "plain.txt.gz"))
    let unpacked = unwrap(gzip_decompress_file("plain.txt.gz", "again.txt"))
    print(int_to_text(packed) + " " + int_to_text(unpacked))
    match gzip_decompress_file("absent.gz", "x.txt"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("missing " + problem.code)
    match gzip_compress_file("plain.txt", "../outside.gz"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("escape " + problem.code)
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains(&format!(" {}", data.len())), "{out}");
    assert!(out.contains("missing "), "{out}");
    assert!(out.contains("escape "), "{out}");
    assert_eq!(std::fs::read(root.join("again.txt")).unwrap(), data);
    assert!(!scratch.0.join("outside.gz").exists());
    python(
        "import gzip, sys\nr = sys.argv[1]\nassert gzip.open(r + '/plain.txt.gz', 'rb').read() == open(r + '/plain.txt', 'rb').read()\n",
        &[root.to_str().unwrap()],
    );
}

#[test]
fn file_variants_are_confined_to_the_granted_root() {
    let scratch = Scratch::new("compress-confined");
    std::fs::write(scratch.0.join("outside.txt"), "outside").unwrap();
    std::fs::write(scratch.root().join("a.txt"), "inside").unwrap();
    let made = Command::new("tar")
        .args(["-cf", "a.tar", "a.txt"])
        .current_dir(scratch.root())
        .status()
        .unwrap();
    assert!(made.success());
    let source = program(
        r#"    match gzip_compress_file("../outside.txt", "a.gz"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("read " + problem.code)
    match gzip_decompress_file("a.txt", "../out.gz"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("write " + problem.code)
    match tar_list("../a.tar"):
        Ok(names):
            print("unexpected ok")
        Err(problem):
            print("list " + problem.code)
    match tar_write("..", "a.tar"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("dir " + problem.code)
    match tar_read("a.tar", "../dest"):
        Ok(count):
            print("unexpected ok")
        Err(problem):
            print("dest " + problem.code)
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    assert_eq!(out.matches(" R_HOST_DENIED").count(), 5, "{out}");
    assert!(!scratch.0.join("out.gz").exists());
    assert!(!scratch.0.join("dest").exists());
}

fn make_tree(root: &Path) {
    std::fs::create_dir_all(root.join("tree/sub/deeper")).unwrap();
    std::fs::write(root.join("tree/a.txt"), "alpha\n").unwrap();
    std::fs::write(root.join("tree/sub/b.txt"), "beta\n".repeat(1000)).unwrap();
    std::fs::write(root.join("tree/sub/deeper/c.bin"), [0u8, 1, 2, 255]).unwrap();
    std::fs::write(root.join("tree/caf\u{e9}.txt"), "unicode name\n").unwrap();
    std::fs::create_dir_all(root.join("tree/empty")).unwrap();
}

#[test]
fn tar_written_by_guion_lists_under_the_system_tar_and_python() {
    let scratch = Scratch::new("tar-write");
    let root = scratch.root();
    make_tree(&root);
    let source = program(
        r#"    let plain = unwrap(tar_write("tree", "out.tar"))
    let packed = unwrap(tar_write("tree", "out.tgz"))
    print(int_to_text(plain) + " " + int_to_text(packed))
    let names = unwrap(tar_list("out.tgz"))
    print(text_join(names, ","))
    assert(unwrap(tar_list("out.tar")) == names)
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(out.starts_with("7 7\n"), "{out}");
    let expected = [
        "a.txt",
        "caf\u{e9}.txt",
        "empty/",
        "sub/",
        "sub/b.txt",
        "sub/deeper/",
        "sub/deeper/c.bin",
    ];
    // Names are sorted by path, directories carry a trailing slash, and the archive root is the
    // directory's contents, so extracting it elsewhere recreates the tree.
    assert!(out.contains(&expected.join(",")), "{out}");

    // The system tar: `tar -tzf` on the compressed archive (macOS bsdtar here; the Debian 12
    // GNU tar check is documented in docs/known-issues.md and not run by this test).
    let listed = Command::new("tar")
        .args(["-tzf", "out.tgz"])
        .current_dir(&root)
        .output()
        .expect("tar starts");
    assert!(
        listed.status.success(),
        "tar -tzf failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let mut system: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(|line| line.trim_start_matches("./").to_owned())
        .collect();
    system.sort();
    let mut want: Vec<String> = expected.iter().map(|name| (*name).to_owned()).collect();
    want.sort();
    assert_eq!(system, want);
    let plain = Command::new("tar")
        .args(["-tf", "out.tar"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(plain.status.success());

    // The system tar extracts it to the same tree.
    std::fs::create_dir_all(root.join("sys")).unwrap();
    let extracted = Command::new("tar")
        .args(["-xzf", "out.tgz", "-C", "sys"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(extracted.success());
    assert_eq!(
        std::fs::read(root.join("sys/sub/b.txt")).unwrap(),
        "beta\n".repeat(1000).into_bytes()
    );
    assert!(root.join("sys/empty").is_dir());

    python(
        "import tarfile, sys\nr = sys.argv[1]\nt = tarfile.open(r + '/out.tgz')\nn = sorted(m.name.rstrip('/') for m in t.getmembers())\nassert n == ['a.txt', 'caf\\u00e9.txt', 'empty', 'sub', 'sub/b.txt', 'sub/deeper', 'sub/deeper/c.bin'], n\n",
        &[root.to_str().unwrap()],
    );
}

#[test]
fn tar_read_round_trips_a_tree_and_reads_system_tar_archives() {
    let scratch = Scratch::new("tar-read");
    let root = scratch.root();
    make_tree(&root);
    let made = Command::new("tar")
        .args(["-czf", "sys.tgz", "-C", "tree", "."])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(made.success());
    let source = program(
        r#"    let first = unwrap(tar_write("tree", "g.tgz"))
    let n = unwrap(tar_read("g.tgz", "back"))
    let m = unwrap(tar_read("sys.tgz", "fromsys"))
    print(int_to_text(first) + " " + int_to_text(n) + " " + int_to_text(m))
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(out.starts_with("7 7 "), "{out}");
    for base in ["back", "fromsys"] {
        assert_eq!(
            std::fs::read(root.join(base).join("sub/b.txt")).unwrap(),
            "beta\n".repeat(1000).into_bytes(),
            "{base}"
        );
        assert_eq!(
            std::fs::read(root.join(base).join("sub/deeper/c.bin")).unwrap(),
            vec![0u8, 1, 2, 255]
        );
        assert!(root.join(base).join("empty").is_dir());
        assert_eq!(
            std::fs::read_to_string(root.join(base).join("caf\u{e9}.txt")).unwrap(),
            "unicode name\n"
        );
    }
}

#[test]
fn tar_read_refuses_paths_and_links_that_escape() {
    let scratch = Scratch::new("tar-escape");
    let root = scratch.root();
    python(
        r#"
import tarfile, io, sys
r = sys.argv[1]
def add(t, name, data=b'x', type=tarfile.REGTYPE, link=''):
    i = tarfile.TarInfo(name)
    i.type = type
    i.linkname = link
    i.size = len(data) if type == tarfile.REGTYPE else 0
    t.addfile(i, io.BytesIO(data) if type == tarfile.REGTYPE else None)
def make(name, entries, gz=False):
    t = tarfile.open(r + '/' + name, 'w:gz' if gz else 'w', format=tarfile.GNU_FORMAT)
    for e in entries:
        add(t, *e)
    t.close()
make('dotdot.tar', [('fine.txt',), ('../evil.txt',)])
make('dotdot-mid.tar', [('a/../../evil.txt',)])
make('absolute.tar', [('/tmp/guion-absolute-evil.txt',)])
make('symlink-up.tar', [('link', b'', tarfile.SYMTYPE, '../outside')])
make('symlink-abs.tar', [('link', b'', tarfile.SYMTYPE, '/etc')])
make('symlink-mid.tar', [('link', b'', tarfile.SYMTYPE, 'a/../..')])
make('hardlink-up.tar', [('link', b'', tarfile.LNKTYPE, '../secret')])
make('through-link.tar', [('d', b'', tarfile.SYMTYPE, 'sub'), ('d/x.txt',)])
make('safe-link.tar', [('real.txt', b'real'), ('alias', b'', tarfile.SYMTYPE, 'real.txt')])
make('dotdot.tgz', [('../evil.txt',)], True)
"#,
        &[root.to_str().unwrap()],
    );
    std::fs::write(scratch.0.join("secret"), "secret").unwrap();
    let source = program(
        r#"    let names = ["dotdot.tar", "dotdot-mid.tar", "absolute.tar", "symlink-up.tar", "symlink-abs.tar", "symlink-mid.tar", "hardlink-up.tar", "dotdot.tgz"]
    for name in names:
        match tar_read(name, "dest-" + name):
            Ok(count):
                print("unexpected ok")
            Err(problem):
                print(name + " " + problem.code)
    # Listing is only a listing: it shows the names as stored and extracts nothing.
    print(text_join(unwrap(tar_list("dotdot.tar")), ","))
"#,
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    for name in [
        "dotdot.tar",
        "dotdot-mid.tar",
        "absolute.tar",
        "symlink-up.tar",
        "symlink-abs.tar",
        "symlink-mid.tar",
        "hardlink-up.tar",
        "dotdot.tgz",
    ] {
        assert!(
            out.contains(&format!("{name} E_TAR_UNSAFE_PATH")),
            "{name}: {out}"
        );
    }
    assert!(out.contains("fine.txt,../evil.txt"), "{out}");
    assert!(!scratch.0.join("evil.txt").exists());
    assert!(!root.join("evil.txt").exists());
    assert!(!Path::new("/tmp/guion-absolute-evil.txt").exists());
    // Nothing from a refused archive is left half-extracted beyond entries read before the bad one.
    assert!(!root.join("dest-dotdot.tar/evil.txt").exists());

    // A relative link that stays below the destination is accepted; so is writing below it.
    let safe = program(
        "    let n = unwrap(tar_read(\"safe-link.tar\", \"safe\"))\n    print(int_to_text(n))\n",
    );
    let out = ok(&run(&scratch, None, &safe));
    assert_eq!(out.trim(), "2");
    assert_eq!(std::fs::read(root.join("safe/real.txt")).unwrap(), b"real");
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(root.join("safe/alias")).unwrap(),
        PathBuf::from("real.txt")
    );
}

#[test]
fn tar_read_refuses_a_bomb_at_the_decompress_ceiling() {
    let scratch = Scratch::new("tar-bomb");
    let root = scratch.root();
    python(
        "import tarfile, io, sys\nr = sys.argv[1]\nt = tarfile.open(r + '/bomb.tgz', 'w:gz')\ni = tarfile.TarInfo('big.bin')\ni.size = 64 * 1024 * 1024\nt.addfile(i, io.BytesIO(bytes(i.size)))\nt.close()\n",
        &[root.to_str().unwrap()],
    );
    assert!(std::fs::metadata(root.join("bomb.tgz")).unwrap().len() < 1_000_000);
    let source = program(
        "    match tar_read(\"bomb.tgz\", \"out\"):\n        Ok(count):\n            print(\"unexpected ok\")\n        Err(problem):\n            print(\"bomb \" + problem.code)\n    match tar_list(\"bomb.tgz\"):\n        Ok(names):\n            print(\"listed \" + int_to_text(len(names)))\n        Err(problem):\n            print(\"list \" + problem.code)\n",
    );
    let out = ok(&run(
        &scratch,
        Some("[runtime]\ndecompress_limit_bytes = 1048576\n"),
        &source,
    ));
    assert!(!out.contains("unexpected"), "{out}");
    assert!(out.contains("bomb E_COMPRESS_LIMIT"), "{out}");
    // Writing at most the ceiling is left behind; the refused entry never completes.
    let written = std::fs::metadata(root.join("out/big.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(written <= 1_048_576, "{written}");
}

#[test]
fn tar_write_refuses_symlinks_and_an_archive_inside_the_directory() {
    let scratch = Scratch::new("tar-write-refuse");
    let root = scratch.root();
    make_tree(&root);
    #[cfg(unix)]
    std::os::unix::fs::symlink("a.txt", root.join("tree/link")).unwrap();
    let source = program(
        "    match tar_write(\"tree\", \"x.tar\"):\n        Ok(count):\n            print(\"link \" + int_to_text(count))\n        Err(problem):\n            print(\"link \" + problem.code)\n    match tar_write(\"tree/sub\", \"tree/sub/self.tar\"):\n        Ok(count):\n            print(\"unexpected ok\")\n        Err(problem):\n            print(\"inside \" + problem.code)\n    match tar_write(\"absent\", \"y.tar\"):\n        Ok(count):\n            print(\"unexpected ok\")\n        Err(problem):\n            print(\"absent \" + problem.code)\n",
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(!out.contains("unexpected"), "{out}");
    #[cfg(unix)]
    assert!(out.contains("link E_TAR_UNSAFE_PATH"), "{out}");
    assert!(out.contains("inside E_TAR_UNSAFE_PATH"), "{out}");
    assert!(out.contains("absent R_FILE_"), "{out}");
}

#[test]
fn tar_list_reports_corrupt_archives_as_typed_errors() {
    let scratch = Scratch::new("tar-corrupt");
    let root = scratch.root();
    std::fs::write(root.join("junk.tar"), vec![7u8; 2048]).unwrap();
    std::fs::write(root.join("short.tar"), b"not a tar").unwrap();
    let source = program(
        "    for name in [\"junk.tar\", \"short.tar\"]:\n        match tar_list(name):\n            Ok(names):\n                print(name + \" ok \" + int_to_text(len(names)))\n            Err(problem):\n                print(name + \" \" + problem.code)\n",
    );
    let out = ok(&run(&scratch, None, &source));
    assert!(out.contains("junk.tar E_TAR_CORRUPT"), "{out}");
    assert!(
        out.contains("short.tar E_TAR_CORRUPT") || out.contains("short.tar ok 0"),
        "{out}"
    );
}

#[test]
#[ignore = "writes 200 MB; run with `cargo test --release -p guion-cli --test stdlib_compress -- --ignored`"]
fn a_two_hundred_megabyte_file_streams_through_gzip_with_bounded_memory() {
    let scratch = Scratch::new("compress-200mb");
    let root = scratch.root();
    {
        let mut file = std::fs::File::create(root.join("big.txt")).unwrap();
        let block = sample(1 << 20);
        for _ in 0..200 {
            file.write_all(&block).unwrap();
        }
    }
    let source = program(
        "    let packed = unwrap(gzip_compress_file(\"big.txt\", \"big.txt.gz\"))\n    let unpacked = unwrap(gzip_decompress_file(\"big.txt.gz\", \"big.out\"))\n    print(int_to_text(packed) + \" \" + int_to_text(unpacked))\n",
    );
    std::fs::write(scratch.0.join("main.gu"), &source).unwrap();
    let flag = if cfg!(target_os = "macos") {
        "-l"
    } else {
        "-v"
    };
    let timed = Command::new("/usr/bin/time")
        .arg(flag)
        .arg(env!("CARGO_BIN_EXE_guion"))
        .args(["run", "--file-root", root.to_str().unwrap(), "main.gu"])
        .current_dir(&scratch.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(
        timed.status.success(),
        "{}",
        String::from_utf8_lossy(&timed.stderr)
    );
    let report = String::from_utf8_lossy(&timed.stderr).into_owned();
    let peak = report
        .lines()
        .find_map(|line| {
            if cfg!(target_os = "macos") {
                line.trim()
                    .strip_suffix("maximum resident set size")
                    .and_then(|value| value.trim().parse::<u64>().ok())
            } else {
                line.trim()
                    .strip_prefix("Maximum resident set size (kbytes):")
                    .and_then(|value| value.trim().parse::<u64>().ok().map(|kib| kib * 1024))
            }
        })
        .expect("a peak resident size is reported");
    println!("peak resident size: {peak} bytes for a 200 MB gzip round trip");
    assert!(
        peak < 64 * 1024 * 1024,
        "peak {peak} bytes is not under 64 MB"
    );
    assert_eq!(
        std::fs::metadata(root.join("big.out")).unwrap().len(),
        200 << 20
    );
    let same = Command::new("cmp")
        .arg(root.join("big.txt"))
        .arg(root.join("big.out"))
        .status()
        .unwrap();
    assert!(same.success(), "the round trip changed the bytes");
}

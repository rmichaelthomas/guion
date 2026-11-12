//! Joining frames in the `frames` package.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct App(PathBuf);

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("guion starts")
}

fn run(name: &str, left: &str, right: &str, body: &str) -> String {
    let dir = std::env::temp_dir().join(format!("guion-frames-join-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let app = App(dir.clone());
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("left.csv"), left).unwrap();
    std::fs::write(dir.join("right.csv"), right).unwrap();
    let mut source = String::from(
        "import \"pkg:frames/src/lib.gu\"\n\nfn code(result: Result[Frame, StdError]) -> Text:\n    match result:\n        Ok(frame):\n            return \"ok\"\n        Err(problem):\n            return problem.code\n\nfn dump(frame: Frame) -> Unit:\n    print(text_join_native(frame_names(frame), \",\"))\n    var row = 0\n    while row < frame_rows(frame):\n        var cells: List[Text] = []\n        for column in frame_columns(frame):\n            set cells = list_push(cells, column_cell_text(column, row))\n        print(text_join_native(cells, \",\"))\n        set row = row + 1\n\nfn main() -> Unit:\n    let left = unwrap(frame_read_csv(\"left.csv\"))\n    let right = unwrap(frame_read_csv(\"right.csv\"))\n",
    );
    for line in body.lines() {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    std::fs::write(dir.join("src/main.gu"), source).unwrap();
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/frames");
    let added = guion(&dir, &["add", "frames", "--path", package.to_str().unwrap()]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
    let ran = guion(&dir, &["run", "src/main.gu"]);
    assert!(
        ran.status.success(),
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    drop(app);
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

const LEFT: &str = "id,k,v\n1,a,10\n2,b,20\n3,,30\n4,c,40\n";
const RIGHT: &str = "k,w,v\nb,x,1\na,y,2\nb,z,3\n,q,4\n";

#[test]
fn inner_join_keeps_left_order_and_never_matches_missing_keys() {
    let out = run(
        "inner",
        LEFT,
        RIGHT,
        "dump(unwrap(frame_join(left, right, [\"k\"], \"inner\")))",
    );
    assert_eq!(
        out,
        "id,k,v,w,v_right\n1,a,10,y,2\n2,b,20,x,1\n2,b,20,z,3\n"
    );
}

#[test]
fn left_join_fills_unmatched_rows_with_missing_values() {
    let out = run(
        "left",
        LEFT,
        RIGHT,
        "let joined = unwrap(frame_join(left, right, [\"k\"], \"left\"))\ndump(joined)\nprint(column_missing(unwrap(frame_column(joined, \"w\"))))",
    );
    assert_eq!(
        out,
        "id,k,v,w,v_right\n1,a,10,y,2\n2,b,20,x,1\n2,b,20,z,3\n3,NA,30,NA,NA\n4,c,40,NA,NA\n2\n"
    );
}

#[test]
fn joins_on_two_keys_match_text_by_value() {
    let out = run(
        "two",
        "a,b,v\nx,1,1\ny,1,2\nx,2,3\n",
        "b,a,w\n2,x,7\n1,y,8\n1,z,9\n1,y,10\n",
        "dump(unwrap(frame_join(left, right, [\"a\", \"b\"], \"left\")))",
    );
    assert_eq!(out, "a,b,v,w\nx,1,1,NA\ny,1,2,8\ny,1,2,10\nx,2,3,7\n");
}

#[test]
fn join_rejects_bad_keys() {
    let out = run(
        "bad",
        LEFT,
        "k,w\n1,2\n",
        "print(code(frame_join(left, right, [\"k\"], \"inner\")))\nprint(code(frame_join(left, right, [\"zz\"], \"inner\")))\nprint(code(frame_join(left, left, [\"k\"], \"outer\")))\nprint(code(frame_join(left, right, [], \"inner\")))",
    );
    assert_eq!(out, "E_FRAME_KIND\nE_FRAME_COLUMN\nE_FRAME_KIND\nE_FRAME_COLUMN\n");
}

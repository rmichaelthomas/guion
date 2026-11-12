//! Sorting and grouping in the `frames` package.

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

fn run(name: &str, csv: &str, body: &str) -> String {
    let dir = std::env::temp_dir().join(format!("guion-frames-group-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let app = App(dir.clone());
    std::fs::write(
        dir.join("guion.toml"),
        "[project]\nname = \"app\"\nversion = \"0.1.0\"\nentry = \"src/main.gu\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("data.csv"), csv).unwrap();
    let mut source = String::from(
        "import \"pkg:frames/src/lib.gu\"\n\nfn code(result: Result[Frame, StdError]) -> Text:\n    match result:\n        Ok(frame):\n            return \"ok\"\n        Err(problem):\n            return problem.code\n\nfn main() -> Unit:\n    let frame = unwrap(frame_read_csv(\"data.csv\"))\n",
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

const DATA: &str = "k,g,x,n\n1,b,2.0,10\n2,a,,20\n3,b,1.0,30\n4,,5.0,40\n5,a,3.0,50\n6,b,1.0,60\n";

#[test]
fn sorts_by_several_keys_stably_with_missing_last() {
    let out = run(
        "sort",
        DATA,
        "print(frame_show(unwrap(frame_sort(frame, [\"g\", \"x\"], [true, false])), 20))\nprint(frame_show(unwrap(frame_sort(frame, [\"x\"], [true])), 20))\nprint(code(frame_sort(frame, [\"nope\"], [true])))\nprint(code(frame_sort(frame, [\"g\"], [true, false])))",
    );
    assert_eq!(
        out,
        concat!(
            "k  g     x   n\n",
            "5  a   3.0  50\n",
            "2  a    NA  20\n",
            "1  b   2.0  10\n",
            "3  b   1.0  30\n",
            "6  b   1.0  60\n",
            "4  NA  5.0  40\n",
            "[6 rows x 4 columns]\n",
            "k  g     x   n\n",
            "3  b   1.0  30\n",
            "6  b   1.0  60\n",
            "1  b   2.0  10\n",
            "5  a   3.0  50\n",
            "4  NA  5.0  40\n",
            "2  a    NA  20\n",
            "[6 rows x 4 columns]\n",
            "E_FRAME_COLUMN\n",
            "E_FRAME_SHAPE\n",
        )
    );
}

#[test]
fn groups_aggregate_present_values_in_first_seen_order() {
    let out = run(
        "group",
        DATA,
        "print(frame_show(unwrap(frame_group(frame, [\"g\"], [\"x:mean\", \"x:count\", \"n:sum\", \"x:min\", \"n:max\"])), 20))",
    );
    assert_eq!(
        out,
        concat!(
            "g               x_mean  x_count  n_sum  x_min  n_max\n",
            "b   1.3333333333333333        3    100    1.0     60\n",
            "a                  3.0        1     70    3.0     50\n",
            "NA                 5.0        1     40    5.0     40\n",
            "[3 rows x 6 columns]\n",
        )
    );
}

#[test]
fn group_by_two_keys_and_all_missing_groups() {
    let csv = "a,b,v\nx,1,\nx,1,\ny,2,4\nx,2,5\n";
    let out = run(
        "two",
        csv,
        "print(frame_show(unwrap(frame_group(frame, [\"a\", \"b\"], [\"v:mean\", \"v:min\", \"v:sum\", \"v:count\"])), 20))",
    );
    assert_eq!(
        out,
        concat!(
            "a  b  v_mean  v_min  v_sum  v_count\n",
            "x  1      NA     NA      0        0\n",
            "y  2     4.0      4      4        1\n",
            "x  2     5.0      5      5        1\n",
            "[3 rows x 6 columns]\n",
        )
    );
}

#[test]
fn group_rejects_bad_aggregations() {
    let out = run(
        "bad",
        DATA,
        "print(code(frame_group(frame, [\"g\"], [\"x:median\"])))\nprint(code(frame_group(frame, [\"g\"], [\"g:sum\"])))\nprint(code(frame_group(frame, [\"zz\"], [\"x:sum\"])))\nprint(code(frame_group(frame, [\"g\"], [\"x\"])))",
    );
    assert_eq!(out, "E_FRAME_KIND\nE_FRAME_KIND\nE_FRAME_COLUMN\nE_FRAME_KIND\n");
}

//! Native-process contracts for the built-in catalog on the command line (TD-155): `catalog`
//! lists it, and `slice` takes catalog ids where it takes profile files.

use std::{
    ffi::OsStr,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let index = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("karasu-cli-catalog-{}-{index}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The catalog's files, as the repository holds them.
fn catalog_file(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/catalog")
        .join(path)
}

/// A closed 10 × 10 × 1 mm box as ASCII STL.
fn box_stl() -> String {
    let (min, max) = ([0.0, 0.0, 0.0], [10.0, 10.0, 1.0]);
    let corner = |i: usize| {
        [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ]
    };
    let quads = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut text = String::from("solid box\n");
    for quad in quads {
        for triangle in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
            text.push_str("facet normal 0 0 0\nouter loop\n");
            for i in triangle {
                let [x, y, z] = corner(i);
                writeln!(text, "vertex {x} {y} {z}").unwrap();
            }
            text.push_str("endloop\nendfacet\n");
        }
    }
    text + "endsolid box\n"
}

fn run_in(directory: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_karasu-cli"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
    run_in(&std::env::temp_dir(), args)
}

fn success(output: &Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn failure(output: &Output) -> String {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output.stdout, [] as [u8; 0]);
    let error = String::from_utf8(output.stderr.clone()).unwrap();
    assert!(!error.contains("panicked"), "{error}");
    error
}

#[test]
fn catalog_lists_the_printers_one_printer_and_the_resins() {
    let printers = success(&run(["catalog"]));
    assert!(
        printers.starts_with("Printers of the built-in catalog ("),
        "{printers}"
    );
    for id in [
        "anycubic-photon-p1",
        "elegoo-saturn-4-ultra-16k",
        "anycubic-photon-p1-max",
    ] {
        assert!(printers.lines().any(|l| l.starts_with(id)), "{id}");
    }
    let mars = success(&run(["catalog", "--printer", "elegoo-mars-5"]));
    assert!(mars.contains("Elegoo Mars 5 (elegoo-mars-5)"), "{mars}");
    assert!(
        mars.lines().any(|l| l.starts_with("elegoo-standard-resin ")
            && l.ends_with("elegoo-mars-5-elegoo-standard-resin")),
        "{mars}"
    );
    let resins = success(&run(["catalog", "--resins"]));
    assert!(resins
        .lines()
        .any(|l| l.starts_with("anycubic-standard-resin ")));
    let unknown = failure(&run(["catalog", "--printer", "saturn-4"]));
    assert!(
        unknown.contains("did you mean elegoo-saturn-4"),
        "{unknown}"
    );
}

#[test]
fn catalog_ids_slice_exactly_as_the_catalog_files() {
    let temp = TempDirectory::new();
    let model = temp.0.join("box.stl");
    fs::write(&model, box_stl()).unwrap();
    let common = |output: &str| {
        vec![
            "slice".into(),
            "--assume-mm".into(),
            model.clone().into_os_string(),
            "--format".into(),
            "goo-v3".into(),
            "--center".into(),
            "--drop".into(),
            "--no-progress".into(),
            "--output".into(),
            temp.0.join(output).into_os_string(),
        ]
    };
    // By ids, the print setting chosen from the catalog for the printer and the resin.
    let mut by_ids = common("ids.goo");
    by_ids.extend(
        [
            "--printer",
            "elegoo-mars-5",
            "--resin",
            "elegoo-standard-resin",
        ]
        .map(Into::into),
    );
    let summary = success(&run(by_ids));
    assert!(
        summary.starts_with(
            "Catalog profiles: printer elegoo-mars-5, resin elegoo-standard-resin, print setting \
             elegoo-mars-5-elegoo-standard-resin (the catalog's for this printer and resin)\n"
        ),
        "{summary}"
    );
    // By the catalog's own files.
    let mut by_files = common("files.goo");
    by_files.extend([
        "--printer".into(),
        catalog_file("printers/elegoo/mars-5.json").into_os_string(),
        "--resin".into(),
        catalog_file("materials/elegoo/standard-resin.json").into_os_string(),
        "--print-setting".into(),
        catalog_file("print-settings/elegoo/mars-5/elegoo-standard-resin.json").into_os_string(),
    ]);
    let summary = success(&run(by_files));
    assert!(!summary.contains("Catalog profiles"), "{summary}");
    assert_eq!(
        fs::read(temp.0.join("ids.goo")).unwrap(),
        fs::read(temp.0.join("files.goo")).unwrap()
    );
}

#[test]
fn ambiguous_unknown_and_uncalibrated_catalog_values_are_refused() {
    let temp = TempDirectory::new();
    fs::write(temp.0.join("box.stl"), box_stl()).unwrap();
    let slice = |printer: &str, resin: &str, extra: &[&str]| {
        let mut args = vec![
            "slice",
            "--assume-mm",
            "box.stl",
            "--format",
            "goo-v3",
            "--center",
            "--drop",
            "--no-progress",
            "--output",
            "out.goo",
            "--printer",
            printer,
            "--resin",
            resin,
        ];
        args.extend(extra);
        run_in(&temp.0, args)
    };
    // A file in the working directory named as a catalog id is not guessed at.
    fs::write(temp.0.join("elegoo-mars-5"), b"{}").unwrap();
    let error = failure(&slice("elegoo-mars-5", "elegoo-standard-resin", &[]));
    assert!(
        error.contains("write ./elegoo-mars-5 for the file"),
        "{error}"
    );
    fs::remove_file(temp.0.join("elegoo-mars-5")).unwrap();
    // An unknown id names close ones.
    let error = failure(&slice("elegoo-mars-6", "elegoo-standard-resin", &[]));
    assert!(error.contains("did you mean"), "{error}");
    // A chosen print setting holds the anti-aliasing, as a given one does.
    let error = failure(&slice(
        "elegoo-mars-5",
        "elegoo-standard-resin",
        &["--antialias", "4"],
    ));
    assert!(error.contains("holds the anti-aliasing"), "{error}");
    // A catalog resin needs a print setting: the catalog's, or one given.
    let error = failure(&slice(
        "anycubic-photon-p1-max",
        "anycubic-standard-resin",
        &[],
    ));
    assert!(
        error.contains("no print setting for resin anycubic-standard-resin"),
        "{error}"
    );
    assert!(!temp.0.join("out.goo").exists());
}

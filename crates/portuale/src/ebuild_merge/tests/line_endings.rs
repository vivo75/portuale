//! #380: the image scan for `\n`/`\r` in paths. Expected values are Portage
//! 3.0.82.2's merge refusals, `LLM/evidence/2026-10-18-380/portage-*.txt`.

use super::*;

fn image(files: &[&str], links: &[(&str, &str)], dirs: &[&str]) -> PathBuf {
    let root = tempdir().join("image");
    for d in dirs {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }
    for (name, target) in links {
        let p = root.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, p).unwrap();
    }
    root
}

#[test]
fn files_with_a_newline_or_return_are_reported_sorted_and_escaped() {
    let img = image(&["usr/c\rd", "usr/a\nb", "usr/plain"], &[], &[]);
    assert_eq!(
        image_paths_with_line_endings(&img).unwrap(),
        vec!["/usr/a\\nb", "/usr/c\\rd"]
    );
}

#[test]
fn a_symlink_whose_target_has_a_newline_is_reported_by_its_own_path() {
    let img = image(&["usr/plain"], &[("usr/lnk", "x\ny")], &[]);
    assert_eq!(
        image_paths_with_line_endings(&img).unwrap(),
        vec!["/usr/lnk"]
    );
}

#[test]
fn an_empty_directory_with_a_newline_is_not_checked() {
    // Portage lists only the `files` of `os.walk`; the `dir` capture merges.
    let img = image(&["usr/plain"], &[], &["usr/d\ne"]);
    assert!(image_paths_with_line_endings(&img).unwrap().is_empty());
}

#[test]
fn a_file_inside_a_directory_with_a_newline_is_reported() {
    let img = image(&["usr/d\ne/f"], &[], &[]);
    assert_eq!(
        image_paths_with_line_endings(&img).unwrap(),
        vec!["/usr/d\\ne/f"]
    );
}

#[test]
fn a_symlink_to_a_directory_is_not_checked() {
    let img = image(&[], &[("usr/lnk", "d\ne")], &["usr/d\ne"]);
    assert!(image_paths_with_line_endings(&img).unwrap().is_empty());
}

#[test]
fn the_refusal_text_is_portages() {
    assert_eq!(
        refusal_lines("app-misc/nlpkg-1", &["/usr/a\\nb".to_string()]),
        vec![
            "This package installs one or more files containing line ending characters:",
            "",
            "\t/usr/a\\nb",
            "",
            "package app-misc/nlpkg-1 NOT merged",
            "",
        ]
    );
}

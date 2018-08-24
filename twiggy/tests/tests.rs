extern crate colored;
extern crate diff;

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

// Add color methods to strings
use colored::Colorize;

fn slurp<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let mut f = fs::File::open(path)?;
    let mut buf = vec![];
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

macro_rules! test {
    ( $name:ident $( , $args:expr )* ) => {
        #[test]
        fn $name() {
            let output = Command::new("cargo")
                .arg("run")
                .arg("--")
                $(
                    .arg($args)
                )*
                .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests"))
                .output()
                .unwrap();

            assert!(
                output.status.success(),
                "should have run `twiggy` OK\n\n\
                 ============================== stdout ==============================\n\n\
                 {}\n\n\
                 ============================== stderr ==============================\n\n\
                 {}\n\n",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );

            let expected_path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/expectations/",
                stringify!($name)
            );

            // Ignore errors. The diffing will provide a better diagnostic report.
            let expected = slurp(expected_path).unwrap_or(vec![]);
            let expected = String::from_utf8_lossy(&expected);
            let expected_lines = expected.lines().collect::<Vec<&str>>();

            let actual = String::from_utf8_lossy(&output.stdout);
            let actual_lines = actual.lines().collect::<Vec<&str>>();

            if actual_lines != expected_lines {
                let mut cmd = "twiggy".to_string();
                $(
                    cmd.push(' ');
                    cmd.push_str($args);
                )*
                println!(
                    "\n{} {}\n",
                    format!("`{}`", cmd).red(),
                    "did not have the expected output!".red()
                );
                println!("--- {}", expected_path);
                println!(
                    "{} {}\n",
                    "+++ actually generated by".red(),
                    format!("`{}`", cmd).red()
                );
                for diff in diff::slice(&expected_lines, &actual_lines) {
                    match diff {
                        diff::Result::Left(l) => {
                            println!("{}", format!("-{}", l).red())
                        }
                        diff::Result::Both(l, _) => println!(" {}", l),
                        diff::Result::Right(r) => {
                            println!("{}", format!("+{}", r).red())
                        }
                    }
                }
                println!();
                panic!();
            }
        }
    }
}

// Top Tests:
// ----------------------------------------------------------------------------

test!(
    top_wee_alloc,
    "top",
    "-n",
    "10",
    "./fixtures/wee_alloc.wasm"
);
test!(top_mappings, "top", "-n", "10", "./fixtures/mappings.wasm");

test!(
    top_retained_wee_alloc,
    "top",
    "-n",
    "10",
    "--retained",
    "./fixtures/wee_alloc.wasm"
);
test!(
    top_retained_mappings,
    "top",
    "-n",
    "10",
    "--retained",
    "./fixtures/mappings.wasm"
);

// This should not fail to open and write `whatever-output.txt`.
test!(
    output_to_file,
    "top",
    "./fixtures/wee_alloc.wasm",
    "-o",
    "whatever-output.txt"
);

test!(
    top_2_json,
    "top",
    "./fixtures/wee_alloc.wasm",
    "-n",
    "2",
    "-f",
    "json"
);

test!(
    top_2_json_retained,
    "top",
    "./fixtures/wee_alloc.wasm",
    "--retained",
    "-n",
    "2",
    "-f",
    "json"
);

test!(
    top_2_csv,
    "top",
    "./fixtures/wee_alloc.wasm",
    "-n",
    "4",
    "-f",
    "csv"
);

test!(
    top_2_csv_retained,
    "top",
    "./fixtures/wee_alloc.wasm",
    "--retained",
    "-n",
    "4",
    "-f",
    "csv"
);

// Dominators Tests:
// ----------------------------------------------------------------------------

test!(
    dominators_wee_alloc,
    "dominators",
    "./fixtures/wee_alloc.wasm"
);

test!(
    dominators_wee_alloc_json,
    "dominators",
    "./fixtures/wee_alloc.wasm",
    "-f",
    "json"
);

test!(
    dominators_wee_alloc_csv,
    "dominators",
    "./fixtures/wee_alloc.wasm",
    "-f",
    "csv"
);

test!(
    dominators_wee_alloc_with_depth_and_row,
    "dominators",
    "./fixtures/wee_alloc.wasm",
    "-d",
    "5",
    "-r",
    "3"
);

test!(
    dominators_wee_alloc_subtree,
    "dominators",
    "./fixtures/wee_alloc.wasm",
    "hello"
);

test!(
    dominators_wee_alloc_subtree_json,
    "dominators",
    "./fixtures/wee_alloc.wasm",
    "-f",
    "json",
    "hello"
);

test!(
    dominators_regex_any_func,
    "dominators",
    "./fixtures/paths_test.wasm",
    "--regex",
    "func\\[[0-9]+\\]"
);

// Paths Tests:
// ----------------------------------------------------------------------------

test!(
    paths_test_called_once,
    "paths",
    "./fixtures/paths_test.wasm",
    "calledOnce"
);

test!(
    paths_test_called_twice,
    "paths",
    "./fixtures/paths_test.wasm",
    "calledTwice"
);

test!(
    paths_test_default_output,
    "paths",
    "./fixtures/paths_test.wasm"
);

test!(
    paths_test_default_output_desc,
    "paths",
    "./fixtures/paths_test.wasm",
    "--descending"
);

test!(
    paths_test_default_output_desc_with_depth,
    "paths",
    "./fixtures/paths_test.wasm",
    "--descending",
    "-d",
    "2"
);

test!(
    paths_wee_alloc,
    "paths",
    "./fixtures/wee_alloc.wasm",
    "wee_alloc::alloc_first_fit::h9a72de3af77ef93f",
    "hello",
    "goodbye"
);

test!(
    paths_wee_alloc_csv,
    "paths",
    "./fixtures/wee_alloc.wasm",
    "wee_alloc::alloc_first_fit::h9a72de3af77ef93f",
    "hello",
    "goodbye",
    "-f",
    "csv"
);

test!(
    paths_wee_alloc_with_depth_and_paths,
    "paths",
    "./fixtures/wee_alloc.wasm",
    "wee_alloc::alloc_first_fit::h9a72de3af77ef93f",
    "hello",
    "goodbye",
    "-d",
    "1",
    "-r",
    "2"
);

test!(
    paths_wee_alloc_json,
    "paths",
    "./fixtures/wee_alloc.wasm",
    "wee_alloc::alloc_first_fit::h9a72de3af77ef93f",
    "hello",
    "goodbye",
    "-d",
    "3",
    "-f",
    "json"
);

test!(
    paths_test_regex_called_any,
    "paths",
    "./fixtures/paths_test.wasm",
    "called.*",
    "--regex"
);

test!(
    paths_test_regex_exports,
    "paths",
    "./fixtures/paths_test.wasm",
    "export \".*\"",
    "--regex"
);

test!(
    paths_test_regex_exports_desc,
    "paths",
    "./fixtures/paths_test.wasm",
    "export \".*\"",
    "--descending",
    "--regex"
);


test!(
    issue_16,
    "paths",
    "./fixtures/mappings.wasm",
    "compute_column_spans"
);

// Monos Tests:
// ----------------------------------------------------------------------------

test!(cpp_monos, "monos", "./fixtures/cpp-monos.wasm");

test!(monos, "monos", "./fixtures/monos.wasm");

test!(
    monos_maxes,
    "monos",
    "./fixtures/monos.wasm",
    "-m",
    "2",
    "-n",
    "1"
);

test!(monos_only_generics, "monos", "./fixtures/monos.wasm", "-g");

test!(
    monos_wasm_csv,
    "monos",
    "./fixtures/monos.wasm",
    "-f",
    "csv"
);

test!(monos_all, "monos", "./fixtures/monos.wasm", "-a");

test!(
    monos_only_all_generics,
    "monos",
    "./fixtures/monos.wasm",
    "-g",
    "-a"
);

test!(
    monos_all_generics,
    "monos",
    "./fixtures/monos.wasm",
    "--all-generics"
);

test!(
    monos_all_monos,
    "monos",
    "./fixtures/monos.wasm",
    "--all-monos"
);

test!(
    monos_json,
    "monos",
    "./fixtures/monos.wasm",
    "-m",
    "2",
    "-n",
    "1",
    "-f",
    "json"
);

// Diff Tests:
// ----------------------------------------------------------------------------

test!(
    diff_wee_alloc,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm"
);

test!(
    diff_wee_alloc_top_5,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "-n",
    "5"
);

test!(
    diff_wee_alloc_json,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "-f",
    "json"
);

test!(
    diff_wee_alloc_json_top_5,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "-f",
    "json",
    "-n",
    "5"
);

test!(
    diff_wee_alloc_csv,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "-f",
    "csv"
);

test!(
    diff_wee_alloc_csv_top_5,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "-f",
    "csv",
    "-n",
    "5"
);

test!(
    diff_test_regex_wee_alloc,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "--regex",
    "(data|type)\\[\\d*\\]"
);

test!(
    diff_test_exact_wee_alloc,
    "diff",
    "./fixtures/wee_alloc.wasm",
    "./fixtures/wee_alloc.2.wasm",
    "hello",
    "goodbye"
);

// Garbage Tests:
// ----------------------------------------------------------------------------

test!(garbage, "garbage", "./fixtures/garbage.wasm");

test!(
    garbage_top_2,
    "garbage",
    "./fixtures/garbage.wasm",
    "-n",
    "2"
);

test!(
    garbage_json,
    "garbage",
    "./fixtures/garbage.wasm",
    "-f",
    "json"
);

test!(
    garbage_top_2_json,
    "garbage",
    "./fixtures/garbage.wasm",
    "-f",
    "json",
    "-n",
    "2"
);

test!(
    garbage_wee_alloc_top_10,
    "garbage",
    "./fixtures/wee_alloc.wasm"
);

test!(
    garbage_wee_alloc_all,
    "garbage",
    "./fixtures/wee_alloc.wasm",
    "-a"
);

test!(
    garbage_wee_alloc_top_10_json,
    "garbage",
    "./fixtures/wee_alloc.wasm",
    "-f",
    "json"
);

test!(
    garbage_wee_alloc_all_json,
    "garbage",
    "./fixtures/wee_alloc.wasm",
    "-f",
    "json",
    "-a"
);

// ELF Tests:
// ----------------------------------------------------------------------------

test!(
    elf_top_25_hello_world_rs,
    "top",
    "-n",
    "25",
    "./fixtures/hello_elf"
);

test!(elf_top_hello_world_rs, "top", "./fixtures/hello_elf");

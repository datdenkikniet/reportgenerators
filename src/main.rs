use std::collections::HashMap;

use cobertura_rs::*;
use quick_xml::Reader;

fn main() -> std::io::Result<()> {
    let file = std::env::args()
        .nth(1)
        .expect("First argument should be the path to the cobertura coverage file.");

    let mut reader = Reader::from_file(file).expect("Failed to open file.");
    let mut state = Parser::new();

    let coverage = state
        .parse(&mut reader)
        .expect("Failed to parse coverage file.");

    let mut classes_by_file = HashMap::new();

    for class in coverage.packages.iter().flat_map(|v| &v.classes) {
        let entry = classes_by_file
            .entry(class.file_name.as_os_str())
            .or_insert(Vec::new());
        entry.push(class);
    }

    let mut total_source_lines = 0;
    for (_, classes) in classes_by_file {
        total_source_lines += classes
            .iter()
            .flat_map(|c| c.lines.iter())
            .map(|l| l.number + 1)
            .max_by(usize::cmp)
            .unwrap_or(0);
    }

    let all_lines = coverage.lines();

    let (tracked_lines, hit_lines) = all_lines
        .fold((0usize, 0usize), |(total_lines, hit_lines), line| {
            (total_lines + 1, hit_lines + (line.hits > 0) as usize)
        });

    let calculated_line_rate = hit_lines as f64 / tracked_lines as f64;

    println!(
        "{}, {}, {}, {}, {}",
        tracked_lines, hit_lines, calculated_line_rate, coverage.line_rate, total_source_lines
    );

    if calculated_line_rate != coverage.line_rate {
        let err = std::io::Error::other(
            "Reported coverage line rate does not match calculated line rate.",
        );
        return Err(err);
    } else {
        println!("Validation OK :)");
    }

    HtmlGenerator::generate_pages(&coverage)?;

    Ok(())
}

use cobertura_rs::*;
use quick_xml::Reader;

fn main() -> Result<(), &'static str> {
    let file = std::env::args()
        .nth(1)
        .expect("First argument should be the path to the cobertura coverage file.");

    let mut reader = Reader::from_file(file).expect("Failed to open file.");
    let mut state = Parser::new();

    let coverage = state
        .parse(&mut reader)
        .expect("Failed to parse coverage file.");

    let all_lines = coverage.lines();

    let (total_lines, hit_lines) = all_lines
        .fold((0usize, 0usize), |(total_lines, hit_lines), line| {
            (total_lines + 1, hit_lines + (line.hits > 0) as usize)
        });

    let calculated_line_rate = hit_lines as f64 / total_lines as f64;

    println!(
        "{}, {}, {}, {}",
        total_lines, hit_lines, calculated_line_rate, coverage.line_rate
    );

    if calculated_line_rate != coverage.line_rate {
        Err("Reported coverage line rate does not match calculated line rate.")
    } else {
        println!("All OK :)");
        Ok(())
    }
}

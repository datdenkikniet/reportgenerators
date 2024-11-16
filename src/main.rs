use cobertura_rs::*;
use quick_xml::Reader;

fn main() {
    let mut reader = Reader::from_file(std::env::args().skip(1).next().unwrap()).unwrap();
    let mut state = Parser::new();

    let coverage = state.parse(&mut reader).unwrap();

    let all_lines = coverage.lines();

    let (total_lines, hit_lines) = all_lines
        .fold((0usize, 0usize), |(total_lines, hit_lines), line| {
            (total_lines + 1, hit_lines + (line.hits > 0) as usize)
        });

    println!(
        "{}, {}, {}, {}",
        total_lines,
        hit_lines,
        (hit_lines as f64 / total_lines as f64),
        coverage.line_rate
    );
}

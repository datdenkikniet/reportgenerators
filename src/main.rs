use cobertura::*;
use quick_xml::Reader;

fn main() {
    let reader = Reader::from_file(std::env::args().skip(1).next().unwrap()).unwrap();
    let mut state = Parser::new();

    let result = state.parse(reader).unwrap();
}

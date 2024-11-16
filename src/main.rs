use std::path::PathBuf;

use cobertura::*;
use quick_xml::{
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader,
};

fn utf8_attr(input: impl AsRef<[u8]>) -> String {
    String::from_utf8_lossy(input.as_ref()).to_string()
}

fn utf8_start(input: impl AsRef<[u8]>) -> String {
    let name = String::from_utf8_lossy(input.as_ref());
    format!("<{name}>")
}

fn utf8_end(input: impl AsRef<[u8]>) -> String {
    let name = String::from_utf8_lossy(input.as_ref());
    format!("</{name}>")
}

macro_rules! set_required_attributes {
    ($set_on:expr, $attributes:expr, $([$str_name:literal, $ty:ty, $field:ident],)*) => {{
        $(
            let mut $field: Option<$ty> = None;
        )*

        for attribute in $attributes {
            let attribute = attribute.map_err(|_| ParserError::FailedToParseAttribute)?;
            let name = attribute.key.as_ref();
            let value = attribute.unescape_value().unwrap();

            $(
                if name == $str_name {
                    $field = Some(value.parse().map_err(|_| ParserError::InvalidValueForAttribute(utf8_attr($str_name)))?);
                }
            )*
        }

        $(
            if let Some(value) = $field {
                $set_on.$field = value;
            } else {
                return Err(ParserError::MissingRequiredAttribute(utf8_attr($str_name)));
            }
        )*
    }}
}

fn main() {
    let mut reader = Reader::from_file(std::env::args().skip(1).next().unwrap()).unwrap();
    let mut buf = Vec::new();
    let mut state = Parser::new();

    while let Ok(value) = reader.read_event_into(&mut buf) {
        let filtered = match value {
            Event::Eof => break,
            Event::Decl(_) => continue,
            Event::Text(text) if text.as_ref().trim_ascii().is_empty() => continue,
            Event::Text(text) => FilteredEvent::Text(text),
            Event::Start(start) => FilteredEvent::Start(start),
            Event::End(end) => FilteredEvent::End(end),
            Event::Empty(start) => FilteredEvent::AttributesOnly(start),
            _ => continue,
        };

        state.consume_event(&filtered).unwrap();
    }

    // println!("{:#?}", &state.inner);
}

#[derive(Debug)]
enum FilteredEvent<'a> {
    Start(BytesStart<'a>),
    Text(BytesText<'a>),
    End(BytesEnd<'a>),
    AttributesOnly(BytesStart<'a>),
}

pub struct Parser {
    inner: Option<ParserInner>,
}

#[derive(Debug)]
pub enum ParserError {
    ExpectedStart(String),
    ExpectedEnd(String),
    ExpectedStartOrEnd(String),
    UnexpectedStart(String),
    UnexpectedEnd(String),
    UnexpectedValue(String),
    FailedToParseAttribute,
    InvalidValueForAttribute(String),
    MissingRequiredAttribute(String),
}

impl Parser {
    pub fn new() -> Self {
        Self { inner: None }
    }

    // TODO: rewrite returning Poll

    fn consume_event(&mut self, event: &FilteredEvent) -> Result<(), ParserError> {
        let result = if let Some(inner) = &mut self.inner {
            inner.consume_event(event)
        } else {
            self.parse_coverage(event)
        };

        if result.is_err() {
            self.inner.take();
        }

        result
    }

    fn parse_coverage(&mut self, event: &FilteredEvent) -> Result<(), ParserError> {
        let start = match event {
            FilteredEvent::Start(start) => start,
            _ => return Err(ParserError::ExpectedStart("<coverage>".to_string())),
        };

        if start.name().as_ref() != b"coverage" {
            return Err(ParserError::UnexpectedStart(utf8_start(start.name())));
        }

        let mut coverage = Coverage::default();
        let attributes = start.attributes();

        set_required_attributes!(
            coverage,
            attributes,
            [b"line-rate", f64, line_rate],
            [b"branch-rate", f64, branch_rate],
            [b"lines-covered", usize, lines_covered],
            [b"lines-valid", usize, lines_valid],
            [b"branches-covered", usize, branches_covered],
            [b"branches-valid", usize, branches_valid],
            [b"complexity", f64, complexity],
            [b"version", String, version],
            [b"timestamp", u64, timestamp],
        );

        self.inner = Some(ParserInner {
            coverage,
            state: State::ParsingCoverage,
            package: Default::default(),
            class: Default::default(),
            method: Default::default(),
            line: Default::default(),
        });

        Ok(())
    }
}

#[derive(Debug)]
pub struct ParserInner {
    coverage: Coverage,
    package: Package,
    class: Class,
    method: Method,
    line: Line,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    ParsingCoverage,
    ParsingSources,
    ParsingSource,
    ParsingPackages,
    ParsingPackage,
    ParsingClasses,
    ParsingClass,
    ParsingClassLines,
    ParsingClassLine,
    ParsingMethods,
    ParsingMethod,
    ParsingMethodLines,
    ParsingMethodLine,
    ParsingMethodLineConditions,
    ParsingClassLineConditions,
    End,
}

macro_rules ! transition {
    (basic($start:expr), $unexpected:ident, $map_name:ident, $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {{
        $(
            if $start.name().as_ref() == $name.as_bytes() {
                $($op;)?
                return Ok(State::$to);
            }
        )*

        return Err(ParserError::$unexpected($map_name($start.name())));
    }};

    (basic_start($start:expr), $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {
        transition!(basic($start), UnexpectedStart, utf8_start, $($name => $to $(with $op)?,)*)
    };

    (basic_end($start:expr), $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {
        transition!(basic($start), UnexpectedEnd, utf8_end, $($name => $to $(with $op)?,)*)
    };
}

impl ParserInner {
    fn consume_event(&mut self, event: &FilteredEvent) -> Result<(), ParserError> {
        let Self {
            coverage,
            state,
            package,
            class,
            method,
            line,
        } = self;

        let next_state = match state {
            State::ParsingCoverage => Self::in_coverage(event),
            State::ParsingSources => Self::in_sources(event),
            State::ParsingSource => Self::in_source(coverage, event),
            State::ParsingPackages => Self::in_packages(package, event),
            State::ParsingPackage => Self::in_package(coverage, package, event),
            State::ParsingClasses => Self::in_classes(package, class, event),
            State::ParsingClass => Self::in_class(event),
            State::ParsingMethods => Self::in_methods(class, method, event),
            State::ParsingMethod => Self::in_method(event),
            State::ParsingMethodLines => Self::in_method_lines(method, line, event),
            State::ParsingMethodLine => Self::in_method_line(event),
            State::ParsingMethodLineConditions => Self::in_method_line_conditions(line, event),
            State::ParsingClassLines => Self::in_class_lines(class, line, event),
            State::ParsingClassLine => Self::in_class_line(event),
            State::ParsingClassLineConditions => Self::in_class_line_conditions(line, event),
            State::End => panic!("Consuming more after end event."),
        }?;

        self.state = next_state;
        Ok(())
    }

    fn in_coverage(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition! {
                    basic_start(start),
                    "sources" => ParsingSources,
                    "packages" => ParsingPackages,
                };
            }
            FilteredEvent::End(end) => {
                if end.name().as_ref() == b"coverage" {
                    Ok(State::End)
                } else {
                    return Err(ParserError::ExpectedEnd(utf8_end(end.name())));
                }
            }
            _ => todo!(),
        }
    }

    fn in_sources(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition!(basic_start(start), "source" => ParsingSource);
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "sources" => ParsingCoverage);
            }
            _ => todo!(),
        }
    }

    fn in_source(coverage: &mut Coverage, event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Text(text) => {
                coverage.sources.push(Source {
                    data: std::str::from_utf8(text.as_ref())
                        .map(String::from)
                        .unwrap_or(String::new()),
                });

                Ok(State::ParsingSource)
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "source" => ParsingSources)
            }
            _ => todo!(),
        }
    }

    fn in_packages(package: &mut Package, event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                if start.name().as_ref() == b"package" {
                    set_required_attributes!(
                        package,
                        start.attributes(),
                        [b"name", String, name],
                        [b"line-rate", f64, line_rate],
                        [b"branch-rate", f64, branch_rate],
                        [b"complexity", f64, complexity],
                    );

                    Ok(State::ParsingPackage)
                } else {
                    Err(ParserError::UnexpectedStart(utf8_start(start.name())))
                }
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "packages" => ParsingCoverage)
            }
            _ => todo!(),
        }
    }

    fn in_package(
        coverage: &mut Coverage,
        package: &mut Package,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition!(basic_start(start), "classes" => ParsingClasses)
            }
            FilteredEvent::End(end) => {
                let package = std::mem::replace(package, Default::default());
                transition!(basic_end(end), "package" => ParsingPackages with coverage.packages.push(package))
            }
            _ => todo!(),
        }
    }

    fn in_classes(
        package: &mut Package,
        class: &mut Class,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                if start.name().as_ref() == b"class" {
                    set_required_attributes!(
                        class,
                        start.attributes(),
                        [b"name", String, name],
                        [b"filename", PathBuf, file_name],
                        [b"line-rate", f64, line_rate],
                        [b"branch-rate", f64, branch_rate],
                        [b"complexity", f64, complexity],
                    );

                    Ok(State::ParsingClass)
                } else {
                    Err(ParserError::UnexpectedStart(utf8_start(start.name())))
                }
            }
            FilteredEvent::End(end) => {
                let class = std::mem::replace(class, Default::default());
                transition!(basic_end(end), "classes" => ParsingPackage with package.classes.push(class))
            }
            _ => todo!(),
        }
    }

    fn in_class(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition! {
                    basic_start(start),
                    "methods" => ParsingMethods,
                    "lines" => ParsingClassLines,
                }
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "class" => ParsingClasses)
            }
            _ => todo!(),
        }
    }

    fn in_methods(
        class: &mut Class,
        method: &mut Method,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                if start.name().as_ref() == b"method" {
                    set_required_attributes!(
                        method,
                        start.attributes(),
                        [b"name", String, name],
                        [b"signature", String, signature],
                        [b"line-rate", f64, line_rate],
                        [b"branch-rate", f64, branch_rate],
                    );

                    Ok(State::ParsingMethod)
                } else {
                    Err(ParserError::UnexpectedStart(utf8_start(start.name())))
                }
            }
            FilteredEvent::End(end) => {
                let method = std::mem::replace(method, Default::default());
                transition!(basic_end(end), "methods" => ParsingClass with class.methods.push(method))
            }
            _ => todo!(),
        }
    }

    fn in_method(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition!(basic_start(start), "lines" => ParsingMethodLines)
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "method" => ParsingMethods)
            }
            _ => todo!(),
        }
    }

    fn lines(
        line: &mut Line,
        lines: &mut Vec<Line>,
        event: &FilteredEvent,
        on_attr_only: State,
        on_list: State,
        on_end: State,
    ) -> Result<State, ParserError> {
        let mut load_lines = |start: &BytesStart| {
            if start.name().as_ref() != b"line" {
                return Err(ParserError::UnexpectedStart(utf8_start(start.name())));
            }

            let attributes = start.attributes();
            let mut number: Option<usize> = None;
            let mut hits: Option<usize> = None;

            let mut _branch = false;
            let mut condition_coverage: Option<String> = None;

            for attribute in attributes {
                let attribute = attribute.map_err(|_| ParserError::FailedToParseAttribute)?;
                let value = attribute.unescape_value().unwrap();

                if attribute.key.as_ref() == b"number" {
                    number = Some(value.parse().map_err(|_| {
                        ParserError::InvalidValueForAttribute(utf8_attr(attribute.key))
                    })?);
                }

                if attribute.key.as_ref() == b"hits" {
                    hits = Some(value.parse().map_err(|_| {
                        ParserError::InvalidValueForAttribute(utf8_attr(attribute.key))
                    })?);
                }

                if attribute.key.as_ref() == b"branch" {
                    _branch = value.parse().map_err(|_| {
                        ParserError::InvalidValueForAttribute(utf8_attr(attribute.key))
                    })?;
                }

                if attribute.key.as_ref() == b"condition-coverage" {
                    condition_coverage = Some(value.to_string());
                }
            }

            if let Some(number) = number {
                line.number = number;
            } else {
                return Err(ParserError::MissingRequiredAttribute("number".to_string()));
            }

            if let Some(hits) = hits {
                line.hits = hits;
            } else {
                return Err(ParserError::MissingRequiredAttribute("hits".to_string()));
            }

            line.condition_coverage = condition_coverage;
            Ok(())
        };

        match event {
            FilteredEvent::Start(start) => {
                load_lines(start)?;
                return Ok(on_list);
            }
            FilteredEvent::AttributesOnly(start) => {
                load_lines(start)?;
                let line = std::mem::replace(line, Default::default());
                lines.push(line);
                return Ok(on_attr_only);
            }
            FilteredEvent::End(end) => {
                if end.name().as_ref() == b"lines" {
                    let line = std::mem::replace(line, Default::default());
                    lines.push(line);
                    Ok(on_end)
                } else {
                    Err(ParserError::UnexpectedEnd(utf8_end(end.name())))
                }
            }
            _ => todo!(),
        }
    }

    fn in_method_lines(
        method: &mut Method,
        line: &mut Line,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        Self::lines(
            line,
            &mut method.lines,
            event,
            State::ParsingMethodLines,
            State::ParsingMethodLine,
            State::ParsingMethod,
        )
    }

    fn in_class_lines(
        class: &mut Class,
        line: &mut Line,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        Self::lines(
            line,
            &mut class.lines,
            event,
            State::ParsingClassLines,
            State::ParsingClassLine,
            State::ParsingClass,
        )
    }
    fn in_method_line(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition!(basic_start(start), "conditions" => ParsingMethodLineConditions);
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "line" => ParsingMethodLines);
            }
            _ => todo!(),
        }
    }

    fn in_class_line(event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Start(start) => {
                transition!(basic_start(start), "conditions" => ParsingClassLineConditions);
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "line" => ParsingClassLines);
            }
            _ => todo!(),
        }
    }

    fn in_line_conditions(
        conditions: &mut Vec<Condition>,
        event: &FilteredEvent,
        on_attr_only: State,
        on_end: State,
    ) -> Result<State, ParserError> {
        match event {
            FilteredEvent::AttributesOnly(start) => {
                if start.name().as_ref() == b"condition" {
                    let mut condition = Condition::default();

                    set_required_attributes!(
                        condition,
                        start.attributes(),
                        [b"type", String, r#type],
                        [b"coverage", String, coverage],
                    );

                    conditions.push(condition);

                    Ok(on_attr_only)
                } else {
                    Err(ParserError::UnexpectedStart(utf8_start(start.name())))
                }
            }
            FilteredEvent::End(end) => {
                if end.name().as_ref() == b"conditions" {
                    Ok(on_end)
                } else {
                    Err(ParserError::UnexpectedEnd(utf8_end(end.name())))
                }
            }
            _ => todo!(),
        }
    }

    fn in_method_line_conditions(
        line: &mut Line,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        Self::in_line_conditions(
            &mut line.conditions,
            event,
            State::ParsingMethodLineConditions,
            State::ParsingMethodLine,
        )
    }

    fn in_class_line_conditions(
        line: &mut Line,
        event: &FilteredEvent,
    ) -> Result<State, ParserError> {
        Self::in_line_conditions(
            &mut line.conditions,
            event,
            State::ParsingClassLineConditions,
            State::ParsingClassLine,
        )
    }
}

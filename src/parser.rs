use std::{io::BufRead, path::PathBuf, task::Poll};

use quick_xml::{
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader,
};

use crate::{Class, Condition, Coverage, Line, Method, Package, ParserError, Source};

#[derive(Debug)]
pub enum FilteredEvent<'a> {
    Start(BytesStart<'a>),
    Text(BytesText<'a>),
    End(BytesEnd<'a>),
    AttributesOnly(BytesStart<'a>),
}

impl<'a> FilteredEvent<'a> {
    pub fn try_from(event: Event<'a>) -> Option<Self> {
        match event {
            Event::Eof => None,
            Event::Decl(_) => None,
            Event::Text(text) if text.as_ref().trim_ascii().is_empty() => None,
            Event::Text(text) => Some(FilteredEvent::Text(text)),
            Event::Start(start) => Some(FilteredEvent::Start(start)),
            Event::End(end) => Some(FilteredEvent::End(end)),
            Event::Empty(start) => Some(FilteredEvent::AttributesOnly(start)),
            _ => None,
        }
    }
}

fn utf8_attr(input: impl AsRef<[u8]>) -> String {
    String::from_utf8_lossy(input.as_ref()).to_string()
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

pub struct Parser {
    inner: Option<ParserInner>,
}

impl Parser {
    pub fn new() -> Self {
        Self { inner: None }
    }

    pub fn reset(&mut self) {
        self.inner.take();
    }

    pub fn parse<R>(&mut self, reader: &mut Reader<R>) -> Result<Coverage, ParserError>
    where
        R: BufRead,
    {
        let mut buf = Vec::new();
        loop {
            let event = reader.read_event_into(&mut buf).unwrap();
            if event == Event::Eof {
                return Err(ParserError::UnexpectedEof);
            }

            let filtered = if let Some(filtered) = FilteredEvent::try_from(event) {
                filtered
            } else {
                continue;
            };

            if let Poll::Ready(result) = self.consume_event(&filtered) {
                break result;
            }
        }
    }

    pub fn consume_event(&mut self, event: &FilteredEvent) -> Poll<Result<Coverage, ParserError>> {
        let result = if let Some(inner) = &mut self.inner {
            inner
                .consume_event(event)
                .map(|v| v.map(|_| std::mem::take(&mut inner.coverage)))
        } else {
            self.parse_coverage(event)?;
            Poll::Pending
        };

        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(value) => {
                self.inner.take();
                Poll::Ready(value)
            }
        }
    }

    fn parse_coverage(&mut self, event: &FilteredEvent) -> Result<(), ParserError> {
        let start = match event {
            FilteredEvent::Start(start) => start,
            evt => return Err(ParserError::start(evt, ["coverage"])),
        };

        if start.name().as_ref() != b"coverage" {
            return Err(ParserError::start(event, ["coverage"]));
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
    (basic($value:expr), $unexpected:ident, $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {{
        $(
            if $value.name().as_ref() == $name.as_bytes() {
                $($op;)?
                return Ok(State::$to);
            }
        )*

        let names = [
            $($name,)*
        ];

        return Err(ParserError::$unexpected($value, names));
    }};

    (basic_start($start:expr), $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {
        transition!(basic($start), start, $($name => $to $(with $op)?,)*)
    };

    (basic_end($start:expr), $($name:literal => $to:ident$( with $op:expr)?),*$(,)?) => {
        transition!(basic($start), end, $($name => $to $(with $op)?,)*)
    };
}

impl ParserInner {
    fn consume_event(&mut self, event: &FilteredEvent) -> Poll<Result<(), ParserError>> {
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

        if self.state == State::End {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
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
                transition!(basic_end(end), "coverage" => End);
            }
            FilteredEvent::AttributesOnly(start) => {
                transition! {
                    basic_start(start),
                    "sources" => ParsingCoverage,
                    "packages" => ParsingCoverage,
                };
            }
            evt => Err(ParserError::start_end(
                evt,
                ["sources", "packages"],
                ["coverage"],
            )),
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
            evt => Err(ParserError::start_end(evt, ["source"], ["sources"])),
        }
    }

    fn in_source(coverage: &mut Coverage, event: &FilteredEvent) -> Result<State, ParserError> {
        match event {
            FilteredEvent::Text(text) => {
                coverage.sources.push(Source {
                    _data: std::str::from_utf8(text.as_ref())
                        .map(String::from)
                        .unwrap_or(String::new()),
                });

                Ok(State::ParsingSource)
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "source" => ParsingSources)
            }
            evt => Err(ParserError::start_end(evt, ["text"], ["source"])),
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
                    Err(ParserError::start(event, ["package"]))
                }
            }
            FilteredEvent::End(end) => {
                transition!(basic_end(end), "packages" => ParsingCoverage)
            }
            evt => Err(ParserError::start_end(evt, ["package"], ["packages"])),
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
                let package = std::mem::take(package);
                transition!(basic_end(end), "package" => ParsingPackages with coverage.packages.push(package))
            }
            FilteredEvent::AttributesOnly(start) => {
                transition!(basic_start(start), "classes" => ParsingPackage)
            }
            evt => Err(ParserError::start_end(evt, ["classes"], ["package"])),
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
                    Err(ParserError::start(event, ["class"]))
                }
            }
            FilteredEvent::End(end) => {
                let class = std::mem::take(class);
                transition!(basic_end(end), "classes" => ParsingPackage with package.classes.push(class))
            }
            evt => Err(ParserError::start_end(evt, ["class"], ["classes"])),
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
            FilteredEvent::AttributesOnly(start) => {
                transition! {
                    basic_start(start),
                    "methods" => ParsingClass,
                    "lines" => ParsingClass,
                }
            }
            evt => Err(ParserError::start_end(evt, ["methods", "lines"], ["class"])),
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
                    Err(ParserError::start(event, ["method"]))
                }
            }
            FilteredEvent::End(end) => {
                let method = std::mem::take(method);
                transition!(basic_end(end), "methods" => ParsingClass with class.methods.push(method))
            }
            evt => Err(ParserError::start_end(evt, ["method"], ["methods"])),
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
            evt => Err(ParserError::start_end(evt, ["lines"], ["method"])),
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
                return Err(ParserError::start(event, ["line"]));
            }

            let attributes = start.attributes();
            let mut number: Option<usize> = None;
            let mut hits: Option<usize> = None;

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
                    line.branch = value.parse().map_err(|_| {
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
                let line = std::mem::take(line);
                lines.push(line);
                return Ok(on_attr_only);
            }
            FilteredEvent::End(end) => {
                if end.name().as_ref() == b"lines" {
                    let line = std::mem::take(line);
                    lines.push(line);
                    Ok(on_end)
                } else {
                    Err(ParserError::end(event, ["lines"]))
                }
            }
            evt => Err(ParserError::start_end(evt, ["line"], ["lines"])),
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
            FilteredEvent::AttributesOnly(start) => {
                transition!(basic_start(start), "conditions" => ParsingMethodLine);
            }
            evt => Err(ParserError::start_end(evt, ["conditions"], ["line"])),
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
            FilteredEvent::AttributesOnly(start) => {
                transition!(basic_start(start), "conditions" => ParsingClassLine);
            }
            evt => Err(ParserError::start_end(evt, ["conditions"], ["line"])),
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
                    Err(ParserError::start(event, ["condition"]))
                }
            }
            FilteredEvent::End(end) => {
                if end.name().as_ref() == b"conditions" {
                    Ok(on_end)
                } else {
                    Err(ParserError::end(event, ["conditions"]))
                }
            }
            evt => Err(ParserError::start_end(evt, ["condition"], ["conditions"])),
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

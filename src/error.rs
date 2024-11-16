use quick_xml::events::{BytesEnd, BytesStart};

use crate::FilteredEvent;

#[derive(Debug)]
pub enum ParserError {
    ExpectedStart {
        got: BasicEvent,
        expected: Vec<String>,
    },
    ExpectedEnd {
        got: BasicEvent,
        expected: Vec<String>,
    },
    ExpectedStartOrEnd {
        got: BasicEvent,
        expected_starts: Vec<String>,
        expected_ends: Vec<String>,
    },
    UnexpectedValue(String),
    FailedToParseAttribute,
    InvalidValueForAttribute(String),
    MissingRequiredAttribute(String),
    UnexpectedEof,
}

impl ParserError {
    pub(crate) fn start(
        got: impl Into<BasicEvent>,
        expected: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self::ExpectedStart {
            got: got.into(),
            expected: expected
                .into_iter()
                .map(|v| v.as_ref().to_string())
                .collect(),
        }
    }

    pub(crate) fn end(
        got: impl Into<BasicEvent>,
        expected: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self::ExpectedStart {
            got: got.into(),
            expected: expected
                .into_iter()
                .map(|v| v.as_ref().to_string())
                .collect(),
        }
    }

    pub(crate) fn start_end(
        got: impl Into<BasicEvent>,
        expected_starts: impl IntoIterator<Item = impl AsRef<str>>,
        expected_ends: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        Self::ExpectedStartOrEnd {
            got: got.into(),
            expected_starts: expected_starts
                .into_iter()
                .map(|v| v.as_ref().to_string())
                .collect(),
            expected_ends: expected_ends
                .into_iter()
                .map(|v| v.as_ref().to_string())
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum BasicEvent {
    Start(String),
    End(String),
    Empty(String),
    Text,
}

impl From<&FilteredEvent<'_>> for BasicEvent {
    fn from(value: &FilteredEvent<'_>) -> Self {
        match value {
            FilteredEvent::Start(bytes_start) => Self::from(bytes_start),
            FilteredEvent::Text(_) => Self::Text,
            FilteredEvent::End(bytes_end) => Self::from(bytes_end),
            FilteredEvent::AttributesOnly(bytes_start) => {
                Self::Start(String::from_utf8_lossy(bytes_start.name().as_ref()).to_string())
            }
        }
    }
}

impl<'a, 'b> From<&'a BytesStart<'b>> for BasicEvent {
    fn from(value: &BytesStart) -> Self {
        Self::Start(String::from_utf8_lossy(value.name().as_ref()).to_string())
    }
}

impl<'a, 'b> From<&'a BytesEnd<'b>> for BasicEvent {
    fn from(value: &'a BytesEnd<'b>) -> Self {
        Self::End(String::from_utf8_lossy(value.name().as_ref()).to_string())
    }
}

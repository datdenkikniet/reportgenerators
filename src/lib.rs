use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub sources: Vec<Source>,
    pub packages: Vec<Package>,

    pub line_rate: f64,
    pub branch_rate: f64,
    pub lines_covered: usize,
    pub lines_valid: usize,
    pub branches_covered: usize,
    pub branches_valid: usize,
    pub complexity: f64,
    pub version: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Source {
    // For now
    pub data: String,
}

#[derive(Debug, Clone, Default)]
pub struct Package {
    pub classes: Vec<Class>,
    pub name: String,
    pub line_rate: f64,
    pub branch_rate: f64,
    pub complexity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Class {
    pub methods: Vec<Method>,
    pub lines: Vec<Line>,
    pub name: String,
    pub file_name: PathBuf,
    pub line_rate: f64,
    pub branch_rate: f64,
    pub complexity: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Method {
    pub lines: Vec<Line>,
    pub name: String,
    pub signature: String,
    pub line_rate: f64,
    pub branch_rate: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    pub conditions: Vec<Condition>,
    pub number: usize,
    pub hits: usize,
    // Almost always in the following form `X% (Y/Z)`
    pub condition_coverage: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Condition {
    pub number: usize,
    pub r#type: String,
    // Always like `X%`?
    pub coverage: String,
}

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use serde::Serialize;

use crate::Coverage;

static HTML_PREFIX: &'static str = include_str!("./prefix.part.html");
static HTML_POSTFIX: &'static str = include_str!("./postfix.part.html");
static CLASS_JS: &'static str = include_str!("./class/class.js");
static CLASS_HTML: &'static str = include_str!("./class/class.html");

pub struct HtmlGenerator;

impl HtmlGenerator {
    fn create_full(path: PathBuf, data: &[u8]) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(data)
    }

    pub fn generate_pages(coverage: &Coverage) -> std::io::Result<()> {
        let output_dir = PathBuf::from("output-rs");

        if !output_dir.exists() {
            std::fs::create_dir(&output_dir)?;
        }

        Self::create_full(output_dir.join("class.js"), CLASS_JS.as_bytes())?;

        let index_html = File::create(output_dir.join("index.html"))?;
        let mut index_html = BufWriter::new(index_html);
        index_html.write_all(HTML_PREFIX.as_bytes())?;

        for class in coverage.packages.iter().flat_map(|c| &c.classes) {
            index_html.write_all(
                format!(
                    "\n\t<p><a href=\"./{}.html\">{}</a></p>",
                    class.name, class.name
                )
                .as_bytes(),
            )?;

            // TODO: sanitze name
            let path = output_dir.join(format!("{}.html", class.name));
            let mut class_file = BufWriter::new(File::create(path)?);
            class_file.write_all(CLASS_HTML.as_bytes())?;

            let class_json_data = ClassJsonData {
                methods: class
                    .methods
                    .iter()
                    .map(|m| Method {
                        name: &m.name,
                        signature: &m.signature,
                        line_coverage: m.line_rate * 100.0,
                        branch_coverage: m.branch_rate * 100.0,
                    })
                    .collect(),
            };

            let data = serde_json::to_string(&class_json_data).unwrap();

            class_file.write_all(b"<script>\nconst class_data = JSON.parse(`")?;
            class_file.write_all(data.as_bytes())?;
            class_file.write_all(b"`);\n</script>")?;
        }

        index_html.write_all(HTML_POSTFIX.as_bytes())?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Method<'a> {
    pub name: &'a str,
    pub signature: &'a str,
    pub line_coverage: f64,
    pub branch_coverage: f64,
}

#[derive(Debug, Serialize)]
pub struct ClassJsonData<'a> {
    pub methods: Vec<Method<'a>>,
}

use std::io::Write;

use razor_slices::{attribute, WriteHtmlValue};

struct Item<'a> {
    label: &'a str,
    completed: bool,
    class: Option<&'a str>,
}

fn render_item(item: &Item<'_>, writer: &mut dyn Write) -> Result<(), Box<dyn std::error::Error>> {
    writer.write_all(b"<input")?;
    attribute("type", "checkbox")?.write_html_to(writer)?;
    attribute("checked", item.completed)?.write_html_to(writer)?;
    attribute("class", item.class)?.write_html_to(writer)?;
    attribute("aria-label", item.label)?.write_html_to(writer)?;
    writer.write_all(b">\n")?;
    Ok(())
}

pub fn render_example() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let items = [
        Item {
            label: "Wash <dishes> & \"cups\"",
            completed: true,
            class: Some(""),
        },
        Item {
            label: "Call 'Grandma' 世界",
            completed: false,
            class: None,
        },
    ];
    let mut output = Vec::new();
    for item in &items {
        render_item(item, &mut output)?;
    }
    assert_eq!(output, include_bytes!("../tests/fixtures/attributes.html"));
    let mut repeated = Vec::new();
    for item in &items {
        render_item(item, &mut repeated)?;
    }
    assert_eq!(output, repeated);
    Ok(output)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::io::stdout().lock().write_all(&render_example()?)?;
    Ok(())
}

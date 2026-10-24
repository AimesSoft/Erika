//! Markdown documents, such as a plugin's `README.md`, as the blocks the
//! interface shows one under another: titles, paragraphs, list items,
//! quotes, code, pictures and the rows of a table. A block keeps its words
//! in inline Markdown — emphasis, code and links — which the interface's
//! text shows natively.

use crate::plugins::store::beside;
use bern::media::MediaSource;
use erika_protocol::erika::{DocBlock, DocKind};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// The blocks of `source`; addresses it names relative to itself resolve
/// beside `base`, where it was read from.
pub fn blocks(source: &str, base: &str) -> Vec<DocBlock> {
    let mut doc = Doc {
        base,
        ..Doc::default()
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(source, options) {
        doc.event(event);
    }
    doc.blocks
}

/// One paragraph of plain words, in inline Markdown.
pub fn paragraph(words: &str) -> Vec<DocBlock> {
    if words.trim().is_empty() {
        return Vec::new();
    }
    vec![block(0, DocKind::Paragraph, escape(words), String::new(), String::new(), 0)]
}

fn block(id: i64, kind: DocKind, text: String, detail: String, marker: String, depth: i64) -> DocBlock {
    let image = matches!(kind, DocKind::Image).then(|| MediaSource::new(text.clone()));
    DocBlock {
        id,
        kind,
        text,
        detail,
        marker,
        depth,
        image,
    }
}

/// Words taken literally where inline Markdown reads marks.
fn escape(words: &str) -> String {
    let mut out = String::with_capacity(words.len());
    for c in words.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[derive(Default)]
struct Doc<'a> {
    base: &'a str,
    blocks: Vec<DocBlock>,
    /// The words of the block being read.
    words: String,
    /// The lists the reading is in: the next number of a numbered one.
    lists: Vec<Option<u64>>,
    /// The marker of the item just begun, for its first block.
    marker: Option<String>,
    quotes: usize,
    code: Option<String>,
    links: Vec<String>,
    /// A picture being read: its address. Its description is not shown.
    image: Option<String>,
    /// Pictures of the block being read, shown after it.
    images: Vec<String>,
    table: Option<Table>,
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    header: bool,
}

impl Doc<'_> {
    fn push(&mut self, kind: DocKind, text: String, detail: String, marker: String, depth: i64) {
        let id = self.blocks.len() as i64;
        self.blocks.push(block(id, kind, text, detail, marker, depth));
    }

    fn resolve(&self, address: &str) -> String {
        if address.starts_with('#') {
            address.to_string()
        } else {
            beside(self.base, address)
        }
    }

    /// End the block being read as `kind`, then its pictures.
    fn flush(&mut self, kind: DocKind) {
        let words = std::mem::take(&mut self.words);
        let words = words.trim();
        if !words.is_empty() {
            let depth = self.lists.len().saturating_sub(1) as i64;
            let kind = if !self.lists.is_empty() && matches!(kind, DocKind::Paragraph) {
                DocKind::Item
            } else if self.quotes > 0 && matches!(kind, DocKind::Paragraph) {
                DocKind::Quote
            } else {
                kind
            };
            let marker = if matches!(kind, DocKind::Item) {
                self.marker.take().unwrap_or_default()
            } else {
                String::new()
            };
            self.push(kind, words.to_string(), String::new(), marker, depth);
        }
        for image in std::mem::take(&mut self.images) {
            self.push(DocKind::Image, image, String::new(), String::new(), 0);
        }
    }

    fn write(&mut self, text: &str) {
        match &mut self.table {
            Some(table) if !table.row.is_empty() || self.words.is_empty() => self.words.push_str(text),
            _ => self.words.push_str(text),
        }
    }

    fn event(&mut self, event: Event) {
        if let Some(code) = &mut self.code {
            match event {
                Event::Text(text) => code.push_str(&text),
                Event::End(TagEnd::CodeBlock) => {
                    let code = self.code.take().unwrap_or_default();
                    let code = code.trim_end_matches('\n').to_string();
                    self.push(DocKind::Code, code, String::new(), String::new(), 0);
                }
                _ => {}
            }
            return;
        }
        if self.image.is_some() {
            if let Event::End(TagEnd::Image) = event {
                let address = self.image.take().unwrap_or_default();
                self.images.push(address);
            }
            return;
        }
        match event {
            Event::Start(Tag::Heading { .. }) | Event::Start(Tag::Paragraph) => self.flush_item(),
            Event::End(TagEnd::Heading(level)) => self.flush(match level {
                HeadingLevel::H1 => DocKind::Title,
                HeadingLevel::H2 => DocKind::Heading,
                _ => DocKind::Subheading,
            }),
            Event::End(TagEnd::Paragraph) => self.flush(DocKind::Paragraph),
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_item();
                self.quotes += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush(DocKind::Paragraph);
                self.quotes = self.quotes.saturating_sub(1);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                self.flush_item();
                self.code = Some(String::new());
            }
            Event::Start(Tag::List(start)) => {
                self.flush_item();
                self.lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                self.flush(DocKind::Paragraph);
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                self.flush(DocKind::Paragraph);
                let depth = self.lists.len();
                self.marker = Some(match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ if depth > 1 => "◦".into(),
                    _ => "•".into(),
                });
            }
            // A tight list's words stand in the item without a paragraph.
            Event::End(TagEnd::Item) => self.flush(DocKind::Paragraph),
            Event::Rule => {
                self.flush(DocKind::Paragraph);
                self.push(DocKind::Rule, String::new(), String::new(), String::new(), 0);
            }
            Event::Start(Tag::Table(_)) => {
                self.flush(DocKind::Paragraph);
                self.table = Some(Table::default());
            }
            Event::Start(Tag::TableHead) => {
                if let Some(table) = &mut self.table {
                    table.header = true;
                }
            }
            Event::End(TagEnd::TableCell) => {
                let cell = std::mem::take(&mut self.words).trim().to_string();
                if let Some(table) = &mut self.table {
                    table.row.push(cell);
                }
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    self.table_blocks(table);
                }
            }
            Event::Start(Tag::Strong) | Event::End(TagEnd::Strong) => self.write("**"),
            Event::Start(Tag::Emphasis) | Event::End(TagEnd::Emphasis) => self.write("*"),
            Event::Start(Tag::Strikethrough) | Event::End(TagEnd::Strikethrough) => self.write("~~"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.links.push(self.resolve(&dest_url));
                self.write("[");
            }
            Event::End(TagEnd::Link) => {
                let address = self.links.pop().unwrap_or_default();
                self.write(&format!("]({address})"));
            }
            Event::Start(Tag::Image { dest_url, .. }) => self.image = Some(self.resolve(&dest_url)),
            Event::Text(text) => self.write(&escape(&text)),
            Event::Code(code) => {
                let fence = if code.contains('`') { "``" } else { "`" };
                self.write(&format!("{fence}{code}{fence}"));
            }
            Event::SoftBreak => self.write(" "),
            Event::HardBreak => self.write("\n"),
            _ => {}
        }
    }

    /// The words an item began with, before a block inside it begins.
    fn flush_item(&mut self) {
        if !self.lists.is_empty() {
            self.flush(DocKind::Paragraph);
        }
    }

    /// A table of two columns reads as terms and what they say, its header
    /// left out; a wider one as its rows, the header's in bold.
    fn table_blocks(&mut self, table: Table) {
        let pairs = table.rows.iter().all(|row| row.len() == 2);
        for (index, row) in table.rows.into_iter().enumerate() {
            let header = table.header && index == 0;
            if pairs {
                if !header {
                    let mut cells = row.into_iter();
                    let term = cells.next().unwrap_or_default();
                    let detail = cells.next().unwrap_or_default();
                    self.push(DocKind::Term, term, detail, String::new(), 0);
                }
            } else {
                let joined = row.join(" · ");
                let text = if header && !joined.is_empty() { format!("**{joined}**") } else { joined };
                self.push(DocKind::Paragraph, text, String::new(), String::new(), 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(blocks: &[DocBlock]) -> Vec<(DocKind, &str, &str, &str)> {
        blocks
            .iter()
            .map(|b| (b.kind.clone(), b.text.as_str(), b.detail.as_str(), b.marker.as_str()))
            .collect()
    }

    #[test]
    fn a_readme_reads_as_blocks() {
        let source = "# 标题\n\n第一段，**加粗**、`代码` 和 [链接](https://x.dev)。\n\n## 规则\n\n| 规则 | 说明 |\n|---|---|\n| 刷屏 | 重复 *字符* |\n\n- 一\n- 二\n  1. 甲\n\n> 引用\n\n```\nlet a = 1;\n```\n\n---\n\n![图](shot.png)\n";
        let blocks = blocks(source, "https://host/plugins/a/README.md");
        assert_eq!(
            kinds(&blocks),
            vec![
                (DocKind::Title, "标题", "", ""),
                (DocKind::Paragraph, "第一段，**加粗**、`代码` 和 [链接](https://x.dev)。", "", ""),
                (DocKind::Heading, "规则", "", ""),
                (DocKind::Term, "刷屏", "重复 *字符*", ""),
                (DocKind::Item, "一", "", "•"),
                (DocKind::Item, "二", "", "•"),
                (DocKind::Item, "甲", "", "1."),
                (DocKind::Quote, "引用", "", ""),
                (DocKind::Code, "let a = 1;", "", ""),
                (DocKind::Rule, "", "", ""),
                (DocKind::Image, "https://host/plugins/a/shot.png", "", ""),
            ]
        );
        assert_eq!(blocks[6].depth, 1);
    }

    #[test]
    fn literal_marks_stay_literal() {
        let blocks = blocks("2*3 = 6, a_b\n", "");
        assert_eq!(blocks[0].text, r"2\*3 = 6, a\_b");
    }
}

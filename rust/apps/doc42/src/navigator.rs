use doc42_core::rich::{Block, FieldKind, Paragraph, Position, State};
use win42::localization::text;

const LIMIT: usize = 1_000;
const GROUPS: [&str; 8] = [
    "Headings",
    "Tables",
    "Images",
    "Bookmarks",
    "Comments",
    "Footnotes",
    "Links",
    "Sections",
];

pub(super) enum Target {
    Text(Position, Position),
    Image(u64),
}

pub(super) struct Entry {
    pub(super) label: String,
    pub(super) target: Option<Target>,
}

fn cleaned(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch != '\u{fffc}')
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(128)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn paragraph_text(paragraph: &Paragraph) -> String {
    paragraph
        .runs
        .iter()
        .flat_map(|run| run.text.chars())
        .take(128)
        .collect()
}

fn numbered(label: &str, number: usize) -> String {
    text(label).replace("%d", &number.to_string())
}

fn target(paragraph: &Paragraph, length: usize) -> Target {
    Target::Text(
        Position {
            paragraph: paragraph.id,
            offset: 0,
        },
        Position {
            paragraph: paragraph.id,
            offset: length,
        },
    )
}

fn paragraphs(block: &Block) -> impl Iterator<Item = &Paragraph> {
    let (paragraph, rows) = match block {
        Block::Paragraph(paragraph) => (Some(paragraph), &[][..]),
        Block::Table(table) => (None, table.rows.as_slice()),
        _ => (None, &[][..]),
    };
    paragraph
        .into_iter()
        .chain(rows.iter().flatten().flat_map(|cell| &cell.paragraphs))
}

fn add(group: &mut Vec<Entry>, label: String, target: Target) {
    if group.len() < LIMIT {
        group.push(Entry {
            label,
            target: Some(target),
        });
    }
}

pub(super) fn entries(state: &State) -> Vec<Entry> {
    let mut groups: [Vec<Entry>; 8] = std::array::from_fn(|_| Vec::new());
    let (mut table_number, mut image_number) = (0, 0);
    for block in &state.blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if let Some(level) = paragraph.style.outline_level.filter(|level| *level < 9) {
                    let title = cleaned(&paragraph_text(paragraph));
                    if !title.is_empty() {
                        let length = paragraph.runs.iter().map(|run| run.text.len()).sum();
                        add(
                            &mut groups[0],
                            format!("{}{title}", "  ".repeat(usize::from(level))),
                            target(paragraph, length),
                        );
                    }
                }
            }
            Block::Table(_) => {
                table_number += 1;
                if let Some(paragraph) =
                    paragraphs(block).find(|p| p.runs.iter().any(|run| !run.text.is_empty()))
                {
                    add(
                        &mut groups[1],
                        numbered("Table %d", table_number),
                        target(paragraph, 0),
                    );
                }
            }
            Block::Image(image) => {
                image_number += 1;
                let title = cleaned(&image.alt);
                let label = if title.is_empty() {
                    numbered("Image %d", image_number)
                } else {
                    title
                };
                add(&mut groups[2], label, Target::Image(image.id));
            }
            Block::SectionBreak => {}
        }
        if groups[6].len() < LIMIT {
            for paragraph in paragraphs(block) {
                let mut offset = 0;
                for run in &paragraph.runs {
                    let end = offset + run.text.len();
                    if let Some(address) = &run.link {
                        if offset < end {
                            let label = cleaned(&run.text);
                            add(
                                &mut groups[6],
                                if label.is_empty() {
                                    cleaned(address)
                                } else {
                                    label
                                },
                                Target::Text(
                                    Position {
                                        paragraph: paragraph.id,
                                        offset,
                                    },
                                    Position {
                                        paragraph: paragraph.id,
                                        offset: end,
                                    },
                                ),
                            );
                        }
                    }
                    offset = end;
                }
            }
        }
    }
    for bookmark in state.bookmarks.iter().take(LIMIT) {
        add(
            &mut groups[3],
            cleaned(&bookmark.name),
            Target::Text(bookmark.start.clone(), bookmark.start.clone()),
        );
    }
    for comment in state.comments.iter().take(LIMIT) {
        add(
            &mut groups[4],
            cleaned(&format!("{}: {}", comment.author, comment.text)),
            Target::Text(comment.start.clone(), comment.end.clone()),
        );
    }
    for (index, note) in state.footnotes.iter().take(LIMIT).enumerate() {
        let label = note
            .paragraphs
            .first()
            .map(paragraph_text)
            .unwrap_or_default();
        add(
            &mut groups[5],
            format!("{}. {}", index + 1, cleaned(&label)),
            Target::Text(note.at.clone(), note.at.clone()),
        );
    }
    for (index, section) in state.sections.iter().take(LIMIT).enumerate() {
        let end = state
            .sections
            .get(index + 1)
            .map_or(state.blocks.len(), |next| next.start_block);
        if let Some(paragraph) = state
            .blocks
            .get(section.start_block..end)
            .into_iter()
            .flatten()
            .flat_map(paragraphs)
            .find(|p| p.runs.iter().any(|run| !run.text.is_empty()))
        {
            add(
                &mut groups[7],
                numbered("Section %d", index + 1),
                target(paragraph, 0),
            );
        }
    }
    let mut result = Vec::new();
    for (name, group) in GROUPS.into_iter().zip(groups) {
        if !group.is_empty() {
            result.push(Entry {
                label: text(name),
                target: None,
            });
            result.extend(group);
        }
    }
    if !state.fields.is_empty() {
        result.push(Entry {
            label: text("Fields"),
            target: None,
        });
        result.extend(state.fields.iter().take(LIMIT).map(|field| {
            let label = match &field.kind {
                FieldKind::PageNumber => text("Page Number"),
                FieldKind::PageCount => text("Page Count"),
                FieldKind::Date => text("Date"),
                FieldKind::Custom(label) => cleaned(label),
            };
            Entry {
                label,
                target: Some(Target::Text(field.at.clone(), field.at.clone())),
            }
        }));
    }
    result
}

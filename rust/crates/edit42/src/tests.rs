use super::*;
use doc42_core::rich::{Annotation, Block, Position};
use serde_json::json;

fn apply(session: &mut Session, request: Value) -> Value {
    session.apply_value(request.clone()).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn snapshot(session: &Session) -> Value {
    serde_json::from_str(&session.snapshot("{}").unwrap()).unwrap()
}

fn sheet_snapshot(session: &Session, sheet: usize) -> Value {
    serde_json::from_str(&session.snapshot(&json!({ "sheet": sheet }).to_string()).unwrap()).unwrap()
}

/// Paragraph texts of a document snapshot.
fn texts(session: &Session) -> Vec<String> {
    snapshot(session)["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["runs"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap()).collect())
        .collect()
}

fn ids(session: &Session) -> Vec<u64> {
    snapshot(session)["blocks"].as_array().unwrap().iter().map(|b| b["id"].as_u64().unwrap()).collect()
}

fn block(session: &Session, index: usize) -> Value {
    snapshot(session)["blocks"][index].clone()
}

/// Displayed text of a cell in the active sheet.
fn shown(session: &Session, cell: &str) -> String {
    let (row, col) = spreadsheet::cell_ref(cell).unwrap();
    snapshot(session)["sheet"]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c[0] == row && c[1] == col)
        .map_or(String::new(), |c| c[2].as_str().unwrap().to_owned())
}

fn input(session: &Session, cell: &str) -> String {
    let (row, col) = spreadsheet::cell_ref(cell).unwrap();
    let snap = snapshot(session);
    snap["sheet"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c[0] == row && c[1] == col)
        .map(|c| c[2].as_str().unwrap().to_owned())
        .unwrap_or_else(|| shown(session, cell))
}

fn style(session: &Session, cell: &str) -> Value {
    let (row, col) = spreadsheet::cell_ref(cell).unwrap();
    snapshot(session)["sheet"]["styles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c[0] == row && c[1] == col)
        .map_or(Value::Null, |c| c[2].clone())
}

/// A blank document with "Report" / "Hello brave world" / "One" / "Two".
fn sample_document() -> Session {
    let mut d = Session::blank("document").unwrap();
    let first = ids(&d)[0];
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": first, "offset": 0 }, "text": "Report\nHello brave world\nOne\nTwo" }));
    d
}

// MARK: - Documents

#[test]
fn blank_document_typing_enter_and_one_undo_step_per_request() {
    let mut d = Session::blank("document").unwrap();
    assert_eq!(d.kind(), "document");
    assert!(!d.can_undo() && !d.is_dirty());
    let first = ids(&d)[0];
    // Consecutive typing at the caret is one Undo step.
    let mut caret = json!({ "id": first, "offset": 0 });
    for ch in ["H", "i", "!"] {
        let result = apply(&mut d, json!({ "command": "insert_text", "at": caret, "text": ch, "typing": true }));
        caret = result["caret"].clone();
    }
    assert_eq!(texts(&d), ["Hi!"]);
    assert_eq!(caret, json!({ "id": first, "offset": 3 }));
    assert!(d.is_dirty());
    // Enter splits the paragraph and ends the typing group.
    let result = apply(&mut d, json!({ "command": "split_paragraph", "at": { "id": first, "offset": 2 } }));
    assert_eq!(texts(&d), ["Hi", "!"]);
    assert_eq!(result["caret"]["offset"], 0);
    assert!(d.undo());
    assert_eq!(texts(&d), ["Hi!"]);
    assert!(d.undo());
    assert_eq!(texts(&d), [""], "the three typed characters were one step");
    assert!(!d.can_undo() && d.can_redo());
    assert!(!d.is_dirty());
    assert!(d.redo() && d.redo());
    assert_eq!(texts(&d), ["Hi", "!"]);
    // Typing elsewhere (not at the previous caret) starts a new step.
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": first, "offset": 0 }, "text": "Oh, ", "typing": true }));
    let second = ids(&d)[1];
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": second, "offset": 1 }, "text": "?", "typing": true }));
    assert_eq!(texts(&d), ["Oh, Hi", "!?"]);
    d.undo();
    assert_eq!(texts(&d), ["Oh, Hi", "!"]);
    // The caret of a state is its selection.
    assert_eq!(snapshot(&d)["selection"]["focus"], json!({ "id": first, "offset": 4 }));
}

#[test]
fn a_batch_is_one_step_and_a_failing_request_changes_nothing() {
    let mut d = sample_document();
    let [title, body, one, two] = ids(&d)[..] else { panic!() };
    let before = snapshot(&d);
    let undo_before = d.can_undo();
    let error = d
        .apply_value(json!([
            { "command": "set_paragraph_style", "ids": [title], "style": "heading1" },
            { "command": "insert_text", "at": { "id": 999, "offset": 0 }, "text": "x" },
        ]))
        .unwrap_err();
    assert!(error.contains("999"), "{error}");
    assert_eq!(snapshot(&d), before);
    assert_eq!(d.can_undo(), undo_before);
    let results = apply(
        &mut d,
        json!([
            { "command": "set_paragraph_style", "ids": [title], "style": "heading1" },
            { "command": "set_paragraph_style", "ids": [one, two], "style": "bullet_list" },
            { "command": "apply_text_style", "from": { "id": body, "offset": 6 }, "to": { "id": body, "offset": 11 }, "bold": true },
            { "command": "set_alignment", "ids": [title], "alignment": "center" },
        ]),
    );
    assert_eq!(results.as_array().unwrap().len(), 4);
    assert_eq!(block(&d, 0)["style"], "heading1");
    assert_eq!(block(&d, 0)["align"], "center");
    assert_eq!(block(&d, 2)["style"], "bullet_list");
    assert_eq!(block(&d, 2)["list"]["id"], block(&d, 3)["list"]["id"], "one list");
    assert_eq!(block(&d, 1)["runs"][1], json!({ "text": "brave", "bold": true, "font": "Arial" }));
    assert!(d.undo());
    assert_eq!(snapshot(&d)["blocks"], before["blocks"], "the whole batch undone at once");
    assert!(d.redo());
    assert_eq!(block(&d, 0)["style"], "heading1");
    // Malformed commands are refused before anything runs.
    assert!(d.apply("{\"command\":\"no_such_command\"}").unwrap_err().contains("Invalid command"));
    assert!(d.apply("not json").is_err());
}

#[test]
fn paragraph_styles_lists_and_text_styles() {
    let mut d = sample_document();
    let [title, body, one, two] = ids(&d)[..] else { panic!() };
    for (style, level) in [("heading2", 1), ("heading3", 2), ("heading1", 0)] {
        apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [title], "style": style }));
        assert_eq!(block(&d, 0)["level"], level);
        assert_eq!(block(&d, 0)["style"], style);
    }
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [one, two], "style": "numbered_list" }));
    assert_eq!(block(&d, 2)["list"]["numbered"], true);
    // Turning the second item into a bullet starts a different list; normal removes the list.
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [two], "style": "bullet_list" }));
    assert_ne!(block(&d, 2)["list"]["id"], block(&d, 3)["list"]["id"]);
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [two], "style": "normal" }));
    assert_eq!(block(&d, 3)["list"], Value::Null);
    assert_eq!(block(&d, 3)["style"], "normal");
    // Heading after a list item removes the list marker.
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [one], "style": "heading2" }));
    assert_eq!(block(&d, 2)["list"], Value::Null);
    // Bold, italic and underline toggle independently and keep the font.
    let range = json!({ "from": { "id": body, "offset": 0 }, "to": { "id": body, "offset": 5 } });
    let mut command = json!({ "command": "apply_text_style", "italic": true, "underline": true });
    command.as_object_mut().unwrap().extend(range.as_object().unwrap().clone());
    apply(&mut d, command);
    assert_eq!(block(&d, 1)["runs"][0], json!({ "text": "Hello", "italic": true, "underline": true, "font": "Arial" }));
    apply(&mut d, json!({ "command": "apply_text_style", "ids": [body], "find": "WORLD", "bold": true }));
    assert_eq!(block(&d, 1)["runs"].as_array().unwrap().last().unwrap()["text"], "world");
    assert_eq!(block(&d, 1)["runs"].as_array().unwrap().last().unwrap()["bold"], true);
    apply(&mut d, json!({ "command": "apply_text_style", "ids": [body], "bold": false, "italic": false, "underline": false }));
    assert_eq!(block(&d, 1)["runs"].as_array().unwrap().len(), 1, "runs merge again");
    // A range over several paragraphs.
    apply(&mut d, json!({ "command": "apply_text_style", "from": { "id": title, "offset": 3 }, "to": { "id": body, "offset": 2 }, "bold": true }));
    assert_eq!(block(&d, 0)["runs"][1]["text"], "ort");
    assert_eq!(block(&d, 1)["runs"][0], json!({ "text": "He", "bold": true, "font": "Arial" }));
    assert!(d.apply_value(json!({ "command": "apply_text_style", "ids": [body] })).is_err(), "nothing to apply");
}

#[test]
fn deleting_across_paragraphs_enter_in_lists_and_typed_style() {
    let mut d = sample_document();
    let [title, body, one, two] = ids(&d)[..] else { panic!() };
    // Backspace at the start of a paragraph joins it to the previous one.
    apply(&mut d, json!({ "command": "delete", "from": { "id": title, "offset": 6 }, "to": { "id": body, "offset": 0 } }));
    assert_eq!(texts(&d), ["ReportHello brave world", "One", "Two"]);
    // Positions may come in either order; the range spans whole paragraphs.
    let result = apply(&mut d, json!({ "command": "delete", "from": { "id": two, "offset": 1 }, "to": { "id": title, "offset": 6 } }));
    assert_eq!(texts(&d), ["Reportwo"]);
    assert_eq!(result["caret"], json!({ "id": title, "offset": 6 }));
    d.undo();
    d.undo();
    assert_eq!(texts(&d), ["Report", "Hello brave world", "One", "Two"]);
    // Typing over a selection, with bold for the typed text.
    let result = apply(&mut d, json!({ "command": "insert_text", "at": { "id": body, "offset": 6 }, "to": { "id": body, "offset": 11 },
        "text": "bold", "marks": { "bold": true } }));
    assert_eq!(result["caret"], json!({ "id": body, "offset": 10 }));
    assert_eq!(block(&d, 1)["runs"][1], json!({ "text": "bold", "bold": true, "font": "Arial" }));
    // Typing after bold text continues in bold.
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": body, "offset": 10 }, "text": "er" }));
    assert_eq!(block(&d, 1)["runs"][1]["text"], "bolder");
    // Enter at the end of a list item continues the list; Enter in the empty item ends it.
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [one, two], "style": "bullet_list" }));
    let result = apply(&mut d, json!({ "command": "split_paragraph", "at": { "id": two, "offset": 3 } }));
    let new = result["caret"]["id"].as_u64().unwrap();
    assert_eq!(block(&d, 4)["style"], "bullet_list");
    apply(&mut d, json!({ "command": "split_paragraph", "at": { "id": new, "offset": 0 } }));
    assert_eq!(texts(&d).len(), 5, "no new paragraph");
    assert_eq!(block(&d, 4)["style"], "normal");
    // Enter at the end of a heading gives a normal paragraph.
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [title], "style": "heading1" }));
    apply(&mut d, json!({ "command": "split_paragraph", "at": { "id": title, "offset": 6 } }));
    assert_eq!(block(&d, 1)["style"], "normal");
    assert_eq!(block(&d, 0)["style"], "heading1");
    // Pasted lines become paragraphs.
    let result = apply(&mut d, json!({ "command": "insert_text", "at": { "id": title, "offset": 0 }, "text": "A\r\nB\nC" }));
    assert_eq!(texts(&d)[..3], ["A", "B", "CReport"]);
    assert_eq!(result["caret"]["offset"], 1);
    // Offsets are characters, not bytes.
    let last = *ids(&d).last().unwrap();
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": last, "offset": 0 }, "text": "Ünïcødé 😀" }));
    apply(&mut d, json!({ "command": "delete", "from": { "id": last, "offset": 8 }, "to": { "id": last, "offset": 9 } }));
    assert_eq!(texts(&d).last().unwrap(), "Ünïcødé ");
    assert!(d.apply_value(json!({ "command": "delete", "from": { "id": last, "offset": 0 }, "to": { "id": last, "offset": 99 } })).is_err());
}

#[test]
fn assistant_commands_outline_read_find_replace_insert_delete() {
    let mut d = sample_document();
    let [title, body, one, two] = ids(&d)[..] else { panic!() };
    apply(&mut d, json!({ "command": "set_paragraph_style", "ids": [title], "style": "heading1" }));
    let outline = apply(&mut d, json!({ "command": "get_outline" }));
    assert_eq!(outline["paragraphs"][0], json!({ "index":0, "id": title, "style": "heading1", "length": 6, "text": "Report" }));
    assert!(!d.can_redo() && d.can_undo());
    let found = apply(&mut d, json!({ "command": "find", "text": "o" }));
    assert_eq!(found["matches"].as_array().unwrap().len(), 5);
    assert_eq!(found["matches"][1], json!({ "id": body, "offset": 4, "context": "Hello brave world" }));
    let read = apply(&mut d, json!({ "command": "read_range", "ids": [body] }));
    assert_eq!(read["paragraphs"][0]["text"], "Hello brave world");
    // Reads add no Undo step.
    d.undo();
    assert_eq!(block(&d, 0)["style"], "normal");
    d.redo();
    let replaced = apply(&mut d, json!({ "command": "replace_text", "find": "o", "replace": "0" }));
    assert_eq!(replaced["replaced"], 4);
    assert_eq!(texts(&d), ["Rep0rt", "Hell0 brave w0rld", "One", "Tw0"]);
    apply(&mut d, json!({ "command": "replace_text", "id": one, "replace": "Uno" }));
    assert_eq!(texts(&d)[2], "Uno");
    let inserted = apply(&mut d, json!({ "command": "insert_paragraphs", "after_id": body, "paragraphs": [
        { "text": "Steps", "style": "heading2" }, { "text": "First", "style": "numbered_list" }, { "text": "Second", "style": "numbered_list" } ] }));
    let new_ids = inserted["ids"].as_array().unwrap();
    assert_eq!(new_ids.len(), 3);
    assert_eq!(texts(&d), ["Rep0rt", "Hell0 brave w0rld", "Steps", "First", "Second", "Uno", "Tw0"]);
    assert_eq!(block(&d, 2)["style"], "heading2");
    assert_eq!(block(&d, 3)["list"]["id"], block(&d, 4)["list"]["id"]);
    apply(&mut d, json!({ "command": "insert_paragraphs", "after_id": 0, "paragraphs": [{ "text": "Top", "style": "normal" }] }));
    assert_eq!(texts(&d)[0], "Top");
    apply(&mut d, json!({ "command": "delete_paragraphs", "ids": [one, two, new_ids[0]] }));
    assert_eq!(texts(&d), ["Top", "Rep0rt", "Hell0 brave w0rld", "First", "Second"]);
    let all = ids(&d);
    apply(&mut d, json!({ "command": "delete_paragraphs", "ids": all }));
    assert_eq!(texts(&d), [""], "a document keeps one empty paragraph");
}

#[test]
fn comments_move_with_their_text_when_paragraphs_split_and_join() {
    let mut d = sample_document();
    let [_, body, ..] = ids(&d)[..] else { panic!() };
    let Session::Document(session) = &mut d else { panic!() };
    session
        .doc
        .transact(|s| {
            let id = s.next_id;
            s.next_id += 1;
            s.comments.push(Annotation {
                id,
                start: Position { paragraph: body, offset: 6 },
                end: Position { paragraph: body, offset: 11 },
                author: "Ann".into(),
                text: "brave?".into(),
                resolved: false,
                date: None,
                initials: None,
            });
            Ok(())
        })
        .unwrap();
    apply(&mut d, json!({ "command": "split_paragraph", "at": { "id": body, "offset": 6 } }));
    let new = ids(&d)[2];
    let Session::Document(session) = &d else { panic!() };
    let comment = &session.doc.state().comments[0];
    assert_eq!((comment.start.paragraph, comment.start.offset), (new, 0));
    assert_eq!((comment.end.paragraph, comment.end.offset), (new, 5));
    apply(&mut d, json!({ "command": "delete", "from": { "id": body, "offset": 6 }, "to": { "id": new, "offset": 0 } }));
    let Session::Document(session) = &d else { panic!() };
    let comment = &session.doc.state().comments[0];
    assert_eq!((comment.start.paragraph, comment.start.offset, comment.end.offset), (body, 6, 11));
    // The comment survives a DOCX round trip at the same place.
    let reopened = Session::open(&d.save("docx", false).unwrap(), "docx").unwrap();
    let Session::Document(session) = &reopened else { panic!() };
    assert_eq!(session.doc.state().comments.len(), 1);
}

#[test]
fn history_is_deeper_than_the_core_default() {
    let mut d = Session::blank("document").unwrap();
    let first = ids(&d)[0];
    for n in 0..40 {
        apply(&mut d, json!({ "command": "insert_text", "at": { "id": first, "offset": n }, "text": "x" }));
    }
    let mut steps = 0;
    while d.undo() {
        steps += 1;
    }
    assert_eq!(steps, 40);
}

#[test]
fn edited_documents_round_trip_through_every_format() {
    let mut d = sample_document();
    let [title, body, one, two] = ids(&d)[..] else { panic!() };
    apply(&mut d, json!([
        { "command": "set_paragraph_style", "ids": [title], "style": "heading1" },
        { "command": "set_paragraph_style", "ids": [one, two], "style": "bullet_list" },
        { "command": "apply_text_style", "from": { "id": body, "offset": 6 }, "to": { "id": body, "offset": 11 }, "bold": true },
        { "command": "apply_text_style", "from": { "id": body, "offset": 12 }, "to": { "id": body, "offset": 17 }, "italic": true },
    ]));
    for format in ["docx", "odt", "rtf", "md", "html"] {
        let bytes = d.save(format, false).unwrap_or_else(|e| panic!("{format}: {e}"));
        let reopened = Session::open(&bytes, format).unwrap_or_else(|e| panic!("{format}: {e}"));
        let texts = texts(&reopened);
        let texts: Vec<&str> = texts.iter().map(String::as_str).filter(|t| !t.is_empty()).collect();
        assert_eq!(texts, ["Report", "Hello brave world", "One", "Two"], "{format}");
        let blocks = snapshot(&reopened)["blocks"].as_array().unwrap().clone();
        let find = |text: &str| blocks.iter().find(|b| b["runs"].as_array().unwrap().iter().any(|r| r["text"].as_str().unwrap().contains(text))).unwrap().clone();
        assert_eq!(find("Report")["style"], "heading1", "{format}");
        assert_eq!(find("One")["style"], "bullet_list", "{format}");
        assert_eq!(find("Two")["list"]["id"], find("One")["list"]["id"], "{format}");
        let runs = find("brave")["runs"].as_array().unwrap().clone();
        assert!(runs.iter().any(|r| r["text"] == "brave" && r["bold"] == true), "{format}: {runs:?}");
        assert!(runs.iter().any(|r| r["text"] == "world" && r["italic"] == true), "{format}: {runs:?}");
    }
    // Plain text keeps text only, so it refuses formatted content and accepts a plain document.
    assert!(d.save("txt", false).is_err());
    let mut plain = Session::blank("document").unwrap();
    let first = ids(&plain)[0];
    apply(&mut plain, json!({ "command": "insert_text", "at": { "id": first, "offset": 0 }, "text": "Line one\nLine two" }));
    let bytes = plain.save("txt", false).unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), "Line one\nLine two");
    assert_eq!(texts(&Session::open(&bytes, "txt").unwrap()), ["Line one", "Line two"]);
    assert!(d.save("pdf", false).is_err());
}

#[test]
fn opened_documents_edit_and_keep_their_package_parts() {
    let mut original = sample_document();
    let bytes = original.save("docx", false).unwrap();
    let mut d = Session::open(&bytes, "docx").unwrap();
    assert!(!d.is_dirty());
    let first = ids(&d)[0];
    apply(&mut d, json!({ "command": "insert_text", "at": { "id": first, "offset": 6 }, "text": " 2026" }));
    assert!(d.is_dirty());
    let saved = d.save("docx", false).unwrap();
    assert_eq!(texts(&Session::open(&saved, "docx").unwrap())[0], "Report 2026");
    d.mark_saved();
    assert!(!d.is_dirty());
    // Another format: refused while Word package parts would be lost, unless the caller accepts dropping them.
    if d.save("odt", false).is_err() {
        assert!(d.save("odt", true).is_ok());
    }
    let snap = snapshot(&d);
    assert_eq!(snap["editable"], true);
    assert_eq!(snap["stats"]["paragraphs"], 4);
    assert!(Session::open(b"not a docx", "docx").is_err());
    assert!(Session::open(b"x", "pdf").is_err());
    let _ = &mut original;
}

#[test]
fn documents_with_tracked_changes_are_view_only() {
    let mut d = sample_document();
    let Session::Document(session) = &mut d else { panic!() };
    session.doc.transact(|s| {
        s.track_changes = true;
        Ok(())
    })
    .unwrap();
    let snap = snapshot(&d);
    assert_eq!(snap["editable"], false);
    assert!(snap["reason"].as_str().unwrap().contains("tracked changes"));
    let first = ids(&d)[0];
    assert!(d.apply_value(json!({ "command": "insert_text", "at": { "id": first, "offset": 0 }, "text": "x" })).is_err());
    assert!(d.apply_value(json!({ "command": "get_outline" })).is_ok(), "reading still works");
}

#[test]
fn tables_and_images_are_shown_but_not_edited() {
    let html = b"<p>Before</p><table><tr><td>A</td><td>B</td></tr></table><p>After</p>";
    let mut d = Session::open(html, "html").unwrap();
    let snap = snapshot(&d);
    let types: Vec<&str> = snap["blocks"].as_array().unwrap().iter().map(|b| b["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["paragraph", "table", "paragraph"]);
    assert_eq!(snap["blocks"][1]["rows"][0][1]["paragraphs"][0]["runs"][0]["text"], "B");
    let before = snap["blocks"][0]["id"].as_u64().unwrap();
    let after = snap["blocks"][2]["id"].as_u64().unwrap();
    // Deleting across the table removes it.
    apply(&mut d, json!({ "command": "delete", "from": { "id": before, "offset": 6 }, "to": { "id": after, "offset": 0 } }));
    let Session::Document(session) = &d else { panic!() };
    assert!(matches!(&session.doc.state().blocks[..], [Block::Paragraph(_)]));
    assert_eq!(texts(&d), ["BeforeAfter"]);
}

// MARK: - Spreadsheets

#[test]
fn blank_workbook_values_formulas_and_row_insertion_shift_references() {
    let mut s = Session::blank("spreadsheet").unwrap();
    assert_eq!(s.kind(), "spreadsheet");
    apply(&mut s, json!({ "command": "set_cells", "range": "A1", "values": [["Item", "Price"], ["Tea", 3.5], ["Cake", 4], ["Total", "=SUM(B2:B3)"]] }));
    assert_eq!(shown(&s, "B4"), "7.5");
    assert_eq!(input(&s, "B4"), "=SUM(B2:B3)");
    assert_eq!(shown(&s, "A3"), "Cake");
    assert!(s.is_dirty());
    // A row inserted inside the summed range moves the formula down and widens its reference.
    apply(&mut s, json!({ "command": "insert_rows", "row": 3, "count": 1 }));
    assert_eq!(input(&s, "B5"), "=SUM(B2:B4)");
    apply(&mut s, json!({ "command": "set_cells", "range": "A3:B3", "values": [["Jam", 2]] }));
    assert_eq!(shown(&s, "B5"), "9.5");
    assert!(s.undo());
    assert_eq!(shown(&s, "B5"), "7.5");
    assert!(s.undo());
    assert_eq!(input(&s, "B4"), "=SUM(B2:B3)");
    assert_eq!(shown(&s, "B5"), "");
    assert!(s.redo() && s.redo());
    assert_eq!(shown(&s, "B5"), "9.5");
    // Columns.
    apply(&mut s, json!({ "command": "insert_columns", "column": "B", "count": 1 }));
    assert_eq!(input(&s, "C5"), "=SUM(C2:C4)");
    apply(&mut s, json!({ "command": "delete_columns", "column": "B", "count": 1 }));
    assert_eq!(input(&s, "B5"), "=SUM(B2:B4)");
    apply(&mut s, json!({ "command": "delete_rows", "row": 3, "count": 1 }));
    assert_eq!(shown(&s, "B4"), "7.5");
    // Clearing keeps nothing behind; inserting beyond the used area changes nothing.
    apply(&mut s, json!({ "command": "clear", "range": "A4:B4" }));
    assert_eq!(shown(&s, "B4"), "");
    assert_eq!(apply(&mut s, json!({ "command": "insert_rows", "row": 500, "count": 2 }))["changed"], false);
    // A single value fills a range; a formula is adjusted per cell.
    apply(&mut s, json!({ "command": "set_cells", "range": "C2:C3", "values": [["=B2*2"]] }));
    assert_eq!(input(&s, "C3"), "=B3*2");
    assert_eq!(shown(&s, "C3"), "8");
    assert!(s.apply_value(json!({ "command": "set_cells", "range": "ZZZZ1", "values": [[1]] })).is_err());
    assert!(s.apply_value(json!({ "command": "insert_rows", "row": 0, "count": 1 })).is_err());
}

#[test]
fn workbook_batches_and_failures() {
    let mut s = Session::blank("spreadsheet").unwrap();
    apply(&mut s, json!([
        { "command": "set_cells", "range": "A1", "values": [[1], [2], [3]] },
        { "command": "set_cells", "range": "A4", "values": [["=SUM(A1:A3)"]] },
        { "command": "set_text_style", "range": "A4", "bold": true },
    ]));
    assert_eq!(shown(&s, "A4"), "6");
    assert_eq!(style(&s, "A4")["bold"], true);
    assert!(s.undo());
    assert_eq!(shown(&s, "A1"), "");
    assert!(!s.can_undo(), "one step for the batch");
    s.redo();
    let before = snapshot(&s);
    assert!(s
        .apply_value(json!([
            { "command": "set_cells", "range": "A1", "values": [[100]] },
            { "command": "sort_range", "range": "A1:A4", "by_column": "Q", "descending": false, "has_header": false },
        ]))
        .is_err());
    assert_eq!(snapshot(&s)["sheet"], before["sheet"]);
    // Reads alone add no history.
    let info = apply(&mut s, json!({ "command": "get_sheet_info" }));
    assert_eq!(info["used_range"], "A1:A4");
    let cells = apply(&mut s, json!({ "command": "read_cells", "range": "A3:A4" }));
    assert_eq!(cells["values"], json!([["3"], ["6"]]));
    assert_eq!(cells["formulas"]["A4"], "=SUM(A1:A3)");
    s.undo();
    assert_eq!(shown(&s, "A1"), "");
}

#[test]
fn number_formats_bold_and_sorting() {
    let mut s = Session::blank("spreadsheet").unwrap();
    apply(&mut s, json!({ "command": "set_cells", "range": "A1", "values": [["Name", "Amount"], ["Pear", 1234.5], ["Apple", 0.25], ["Fig", 7]] }));
    apply(&mut s, json!({ "command": "set_number_format", "range": "B2:B4", "format": "#,##0.00" }));
    assert_eq!(shown(&s, "B2"), "1,234.50");
    apply(&mut s, json!({ "command": "set_number_format", "range": "B2:B4", "format": "#,##0.00 \"€\"" }));
    assert_eq!(shown(&s, "B2"), "1,234.50 €");
    apply(&mut s, json!({ "command": "set_number_format", "range": "B2:B4", "format": "\"$\"#,##0.00" }));
    assert_eq!(shown(&s, "B3"), "$0.25");
    apply(&mut s, json!({ "command": "adjust_decimals", "range": "B2:B4", "delta": 1 }));
    assert_eq!(shown(&s, "B3"), "$0.250");
    assert_eq!(style(&s, "B3")["format"], "\"$\"#,##0.000");
    apply(&mut s, json!({ "command": "set_number_format", "range": "B3", "format": "0%" }));
    assert_eq!(shown(&s, "B3"), "25%");
    apply(&mut s, json!({ "command": "adjust_decimals", "range": "B3", "delta": 1 }));
    assert_eq!(shown(&s, "B3"), "25.0%");
    apply(&mut s, json!({ "command": "set_number_format", "range": "B2:B4", "format": "General" }));
    assert_eq!(shown(&s, "B2"), "1234.5");
    apply(&mut s, json!({ "command": "adjust_decimals", "range": "B2", "delta": 1 }));
    assert_eq!(shown(&s, "B2"), "1234.50");
    apply(&mut s, json!({ "command": "adjust_decimals", "range": "B2", "delta": -1 }));
    apply(&mut s, json!({ "command": "adjust_decimals", "range": "B2", "delta": -1 }));
    assert_eq!(style(&s, "B2")["format"], "0", "no decimals left");
    // Bold keeps the number format.
    apply(&mut s, json!({ "command": "set_text_style", "range": "A1:B1", "bold": true }));
    apply(&mut s, json!({ "command": "set_text_style", "range": "B2", "italic": true }));
    assert_eq!(style(&s, "B2"), json!({ "bold": false, "italic": true, "underline": false, "format": "0" }));
    assert_eq!(style(&s, "A1")["bold"], true);
    // Bold on an empty cell outside the used area grows it, so typed text there is bold.
    apply(&mut s, json!({ "command": "set_text_style", "range": "D8", "bold": true }));
    assert_eq!(style(&s, "D8")["bold"], true);
    apply(&mut s, json!({ "command": "set_cells", "range": "D8", "values": [["bold text"]] }));
    assert_eq!(style(&s, "D8")["bold"], true);
    // Sort by name under the header; formats and styles move with their rows.
    apply(&mut s, json!({ "command": "sort_range", "range": "A1:B4", "by_column": "A", "descending": false, "has_header": true }));
    let names: Vec<String> = ["A2", "A3", "A4"].iter().map(|c| shown(&s, c)).collect();
    assert_eq!(names, ["Apple", "Fig", "Pear"]);
    assert_eq!(shown(&s, "A1"), "Name");
    apply(&mut s, json!({ "command": "sort_range", "range": "A1:B4", "by_column": "B", "descending": true, "has_header": true }));
    assert_eq!(shown(&s, "A2"), "Pear");
}

#[test]
fn workbooks_round_trip_and_commands_target_other_sheets() {
    let mut s = Session::blank("spreadsheet").unwrap();
    apply(&mut s, json!({ "command": "set_cells", "range": "A1", "values": [["Qty", "Price", "Sum"], [2, 1.5, "=A2*B2"]] }));
    let csv = s.save("csv", false).unwrap();
    assert_eq!(String::from_utf8(csv.clone()).unwrap(), "Qty,Price,Sum\n2,1.5,=A2*B2\n");
    assert_eq!(shown(&Session::open(&csv, "csv").unwrap(), "C2"), "3");
    apply(&mut s, json!([
        { "command": "set_text_style", "range": "A1:C1", "bold": true },
        { "command": "set_number_format", "range": "C2", "format": "\"€\"0.00" },
    ]));
    assert!(s.save("csv", false).is_err(), "CSV refuses formatting");
    for format in ["xlsx", "ods"] {
        let bytes = s.save(format, false).unwrap();
        let reopened = Session::open(&bytes, format).unwrap();
        assert_eq!(shown(&reopened, "C2"), "€3.00", "{format}");
        assert_eq!(input(&reopened, "C2"), "=A2*B2", "{format}");
        assert_eq!(style(&reopened, "A1")["bold"], true, "{format}");
        // An opened workbook is edited the same way and saved again.
        let mut edited = reopened;
        apply(&mut edited, json!({ "command": "set_cells", "range": "A2", "values": [[4]] }));
        assert_eq!(shown(&edited, "C2"), "€6.00");
        let again = Session::open(&edited.save(format, false).unwrap(), format).unwrap();
        assert_eq!(shown(&again, "C2"), "€6.00", "{format}");
    }
    // A second sheet, addressed by index without making it active.
    let fixture = include_bytes!("../../../../web/test/fixtures/sheets.xlsx");
    let mut book = Session::open(fixture, "xlsx").unwrap();
    let snap = sheet_snapshot(&book, 1);
    assert_eq!(snap["sheets"], json!([{ "name": "November" }, { "name": "October" }]));
    assert_eq!(snap["active"], 0);
    apply(&mut book, json!({ "command": "set_cells", "sheet": 1, "range": "D1", "values": [["Note"]] }));
    let snap = sheet_snapshot(&book, 1);
    assert!(snap["sheet"]["cells"].as_array().unwrap().iter().any(|c| c[2] == "Note"));
    assert_eq!(snap["active"], 0, "the active sheet stays");
    let first = sheet_snapshot(&book, 0);
    assert!(!first["sheet"]["cells"].as_array().unwrap().iter().any(|c| c[2] == "Note"));
    book.undo();
    assert!(!sheet_snapshot(&book, 1)["sheet"]["cells"].as_array().unwrap().iter().any(|c| c[2] == "Note"));
    assert!(book.apply_value(json!({ "command": "clear", "sheet": 5, "range": "A1" })).is_err());
}

#[test]
fn xls_workbooks_are_read_only() {
    let mut s = Session::blank("spreadsheet").unwrap();
    assert_eq!(snapshot(&s)["editable"], true);
    // No XLS writer exists to build a fixture; the read-only flag comes from the core and is reported as is.
    let Session::Spreadsheet(session) = &mut s else { panic!() };
    assert!(!session.workbook().is_read_only());
}

#[test]
fn a1_references_and_column_names() {
    assert_eq!(spreadsheet::cell_ref("B3").unwrap(), (2, 1));
    assert_eq!(spreadsheet::cell_ref("$AA$10").unwrap(), (9, 26));
    assert_eq!(spreadsheet::cell_ref("xfd1048576").unwrap(), (1_048_575, 16_383));
    assert!(spreadsheet::cell_ref("A0").is_err());
    assert!(spreadsheet::cell_ref("XFE1").is_err());
    assert!(spreadsheet::cell_ref("3B").is_err());
    let r = spreadsheet::range_ref("C4:A1").unwrap();
    assert_eq!((r.top, r.left, r.bottom, r.right), (0, 0, 3, 2));
    assert_eq!(column_name(0), "A");
    assert_eq!(column_name(25), "Z");
    assert_eq!(column_name(26), "AA");
    assert_eq!(column_name(16_383), "XFD");
}

#[test]
fn sessions_report_status_and_refuse_unknown_kinds() {
    assert!(Session::blank("presentation").is_err());
    let s = Session::blank("document").unwrap();
    assert_eq!(s.status(), r#"{"canRedo":false,"canUndo":false,"dirty":false}"#);
    assert!(s.snapshot("not json").is_err());
}

#[test]
fn assistant_pages_long_selections_without_splitting_unicode() {
    let mut doc = Session::blank("document").unwrap();
    let body = format!("{}😀конец", "я".repeat(5000));
    let mut paragraphs: Vec<_> = (0..200).map(|i| json!({"text":format!("Section {i}"),"style":"heading1"})).collect();
    paragraphs.push(json!({"text":body,"style":"normal"}));
    let added = apply(&mut doc, json!({"command":"insert_paragraphs","after_id":1,"paragraphs":paragraphs}));
    let id = added["ids"][200].as_u64().unwrap();
    let context = apply(
        &mut doc,
        json!({"command":"document_context","start":{"id":id,"offset":0},"end":{"id":id,"offset":5006}}),
    );
    assert!(context.to_string().len() < 8192);
    assert_eq!(context["headings_omitted"], 194);
    assert_eq!(context["section"]["text"], "Section 199");
    assert_eq!(context["selection"]["truncated"], true);
    let page = apply(&mut doc, json!({"command":"get_outline","offset":199,"limit":2}));
    assert_eq!(page["next_offset"], 201);
    let tail = apply(&mut doc, json!({"command":"get_outline","offset":201,"limit":2}));
    assert!(tail["next_offset"].is_null());
    assert_eq!(tail["paragraphs"][0]["id"], id);
    let read = apply(&mut doc, json!({"command":"read_range","ids":[id],"start":4999,"length":2}));
    assert_eq!(read["paragraphs"][0]["text"], "я😀");
    assert_eq!(read["paragraphs"][0]["next_start"], 5001);
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

#[test]
fn assistant_reads_wide_sheets_by_address_and_aggregates_in_the_engine() {
    let mut book = Session::open(b"amount,flag\n10,x\n20,y", "csv").unwrap();
    let labels: Vec<_> = (1..=14).map(|i| json!([format!("Lot-{i}")])).collect();
    apply(&mut book, json!({"command":"set_cells","range":"IF1","values":labels}));
    apply(&mut book, json!({"command":"set_cells","range":"IF15","values":[["😀abcdef"]]}));
    let context = apply(&mut book, json!({"command":"get_sheet_info","focus":"IF14:IF15"}));
    assert_eq!(context["used_range"], "A1:IF15");
    assert_eq!(context["selection"], "IF14:IF15");
    assert!(context["near_selection"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["address"] == "IF15" && v["value"] == "😀abcdef"));
    assert!(context.to_string().len() < 8192);
    let first = apply(&mut book, json!({"command":"find_cells","text":"lot-","range":"IF1:IF15"}));
    assert_eq!(first["next_after"], "IF12");
    let last = apply(&mut book, json!({"command":"find_cells","text":"lot-","after":"IF12"}));
    assert_eq!(last["matches"][1]["address"], "IF14");
    assert!(last["next_after"].is_null());
    let fragment = apply(&mut book, json!({"command":"read_cells","range":"IF15","length":2}));
    assert_eq!(fragment["values"][0][0], "😀a");
    assert_eq!(fragment["next_starts"]["IF15"], 2);
    let rest = apply(&mut book, json!({"command":"read_cells","range":"IF15","start":2,"length":10}));
    assert_eq!(rest["values"][0][0], "bcdef");
    assert_eq!(rest["next_starts"], json!({}));
    assert_eq!(apply(&mut book, json!({"command":"aggregate_cells","range":"A1:A3","function":"SUM"}))["value"], 30.0);
    assert!(book.undo());
    assert_eq!(shown(&book, "IF15"), "");
}

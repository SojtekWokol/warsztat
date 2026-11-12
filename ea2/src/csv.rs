//! CSV the way the game's tables and Python's csv module use it: "," and "\"", quoted fields may
//! contain newlines, every field written quoted, rows ended with "\n".
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_q = false;
    // at the start of a field: only there a quote opens a quoted field (in ` ""` the quotes are text)
    let mut fresh = true;
    // the current row has content (a blank line is an empty row)
    let mut any = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if fresh => {
                in_q = true;
                fresh = false;
                any = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                fresh = true;
                any = true;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if any {
                    row.push(std::mem::take(&mut field));
                }
                rows.push(std::mem::take(&mut row));
                fresh = true;
                any = false;
            }
            _ => {
                field.push(c);
                fresh = false;
                any = true;
            }
        }
    }
    if any {
        row.push(field);
        rows.push(row);
    }
    rows
}

pub fn write(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for r in rows {
        let cells: Vec<String> = r.iter().map(|f| format!("\"{}\"", f.replace('"', "\"\""))).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

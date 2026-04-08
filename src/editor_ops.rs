// ── Line operations ─────────────────────────────────────────────────────────

/// Delete the line at `cursor_line`. Returns the new cursor line.
pub fn delete_line(lines: &mut Vec<String>, cursor_line: usize) -> usize {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    lines.remove(target);
    if lines.is_empty() {
        lines.push(String::new());
    }
    target
}

/// Swap `cursor_line` with the line above. Returns the new cursor line,
/// or `None` if already at the top.
pub fn move_line_up(lines: &mut [String], cursor_line: usize) -> Option<usize> {
    if cursor_line == 0 {
        return None;
    }
    let target = cursor_line.min(lines.len().saturating_sub(1));
    lines.swap(target, target - 1);
    Some(target - 1)
}

/// Swap `cursor_line` with the line below. Returns the new cursor line,
/// or `None` if already at the bottom.
pub fn move_line_down(lines: &mut [String], cursor_line: usize) -> Option<usize> {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    if target + 1 >= lines.len() {
        return None;
    }
    lines.swap(target, target + 1);
    Some(target + 1)
}

/// Duplicate the line at `cursor_line`. Returns the new cursor line.
pub fn duplicate_line(lines: &mut Vec<String>, cursor_line: usize) -> usize {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    let dup = lines[target].clone();
    lines.insert(target + 1, dup);
    target + 1
}

// ── Comment toggling ────────────────────────────────────────────────────────

/// Toggle line comments on `lines[first..=last]` using `prefix` (e.g. "//").
/// Returns `(cursor_line, new_cursor_col)`.
pub fn toggle_comment(
    lines: &mut [String],
    first: usize,
    last: usize,
    cursor_line: usize,
    cursor_col: usize,
    prefix: &str,
) -> (usize, usize) {
    let all_commented = (first..=last).all(|i| {
        let trimmed = lines.get(i).map_or("", |line| line.trim_start());
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });

    for (i, line) in lines.iter_mut().enumerate() {
        if i < first || i > last {
            continue;
        }

        if all_commented {
            let ws_len = line.len() - line.trim_start().len();
            let rest = &line[ws_len..];
            if let Some(after) = rest.strip_prefix(prefix) {
                let after = after.strip_prefix(' ').unwrap_or(after);
                *line = format!("{}{after}", &line[..ws_len]);
            }
        } else if !line.trim_start().is_empty() {
            let ws_len = line.len() - line.trim_start().len();
            *line = format!("{}{prefix} {}", &line[..ws_len], &line[ws_len..]);
        }
    }

    let delta = prefix.len() + 1;
    let cursor_col = if all_commented {
        cursor_col.saturating_sub(delta)
    } else {
        cursor_col + delta
    };

    (cursor_line, cursor_col)
}

/// Map a file extension to its line-comment prefix.
pub fn comment_prefix(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" | "js" | "ts" | "jsx" | "tsx" | "c" | "cpp" | "cc" | "h" | "hpp" | "java" | "go"
        | "cs" | "swift" | "kt" | "kts" | "scala" | "zig" | "v" | "sv" | "d" | "groovy"
        | "jsonc" | "json5" | "scss" | "less" | "proto" => Some("//"),
        "py" | "sh" | "bash" | "zsh" | "fish" | "rb" | "pl" | "pm" | "r" | "jl" | "yaml"
        | "yml" | "toml" | "conf" | "cfg" | "ini" | "cmake" | "mk" | "tcl" | "awk" | "sed"
        | "ps1" | "elixir" | "ex" | "exs" | "nim" | "cr" | "gd" => Some("#"),
        "lua" | "hs" | "sql" | "ada" | "adb" | "ads" | "vhdl" | "vhd" => Some("--"),
        "lisp" | "cl" | "el" | "clj" | "cljs" | "scm" | "rkt" => Some(";;"),
        "vim" => Some("\""),
        "tex" | "sty" | "cls" | "bib" | "erl" | "hrl" => Some("%"),
        "bat" | "cmd" => Some("REM"),
        "asm" | "s" => Some(";"),
        "f90" | "f95" | "f03" | "f08" => Some("!"),
        _ => None,
    }
}

// ── Case transforms ─────────────────────────────────────────────────────────

fn transform_case_chars(chars: &[char], uppercase: bool) -> String {
    let mut transformed = String::new();
    for &ch in chars {
        if uppercase {
            transformed.extend(ch.to_uppercase());
        } else {
            transformed.extend(ch.to_lowercase());
        }
    }
    transformed
}

pub fn transform_case_range(
    lines: &mut [String],
    from_line: usize,
    from_col: usize,
    to_line: usize,
    to_col: usize,
    uppercase: bool,
) {
    if from_line >= lines.len() || to_line >= lines.len() {
        return;
    }

    if from_line == to_line {
        let chars: Vec<char> = lines[from_line].chars().collect();
        let start = from_col.min(chars.len());
        let end = (to_col + 1).min(chars.len());
        let mut transformed = chars[..start].iter().collect::<String>();
        transformed.push_str(&transform_case_chars(&chars[start..end], uppercase));
        transformed.push_str(&chars[end..].iter().collect::<String>());
        lines[from_line] = transformed;
        return;
    }

    for (line_idx, line) in lines
        .iter_mut()
        .enumerate()
        .take(to_line + 1)
        .skip(from_line)
    {
        let chars: Vec<char> = line.chars().collect();
        let transformed = if line_idx == from_line {
            let start = from_col.min(chars.len());
            let mut line = chars[..start].iter().collect::<String>();
            line.push_str(&transform_case_chars(&chars[start..], uppercase));
            line
        } else if line_idx == to_line {
            let end = (to_col + 1).min(chars.len());
            let mut line = transform_case_chars(&chars[..end], uppercase);
            line.push_str(&chars[end..].iter().collect::<String>());
            line
        } else {
            transform_case_chars(&chars, uppercase)
        };
        *line = transformed;
    }
}

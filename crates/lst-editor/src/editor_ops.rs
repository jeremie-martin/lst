// Line operations

pub fn delete_line(lines: &mut Vec<String>, cursor_line: usize) -> usize {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    lines.remove(target);
    if lines.is_empty() {
        lines.push(String::new());
    }
    target
}

pub fn move_line_up(lines: &mut [String], cursor_line: usize) -> Option<usize> {
    if cursor_line == 0 {
        return None;
    }
    let target = cursor_line.min(lines.len().saturating_sub(1));
    lines.swap(target, target - 1);
    Some(target - 1)
}

pub fn move_line_down(lines: &mut [String], cursor_line: usize) -> Option<usize> {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    if target + 1 >= lines.len() {
        return None;
    }
    lines.swap(target, target + 1);
    Some(target + 1)
}

pub fn duplicate_line(lines: &mut Vec<String>, cursor_line: usize) -> usize {
    let target = cursor_line.min(lines.len().saturating_sub(1));
    let dup = lines[target].clone();
    lines.insert(target + 1, dup);
    target + 1
}

// Indent / outdent
//
// TODO(vim): wire `>>` / `<<` operators to `indent_lines` / `outdent_lines`
// when adding visual-line indent ops to the Vim state machine.

pub fn indent_lines(lines: &mut [String], first: usize, last: usize, width: usize) {
    if first > last || last >= lines.len() {
        return;
    }
    let prefix: String = " ".repeat(width);
    for line in &mut lines[first..=last] {
        line.insert_str(0, &prefix);
    }
}

pub fn outdent_lines(lines: &mut [String], first: usize, last: usize, width: usize) -> Vec<usize> {
    if first > last || last >= lines.len() {
        return Vec::new();
    }
    let mut removed = Vec::with_capacity(last - first + 1);
    for line in &mut lines[first..=last] {
        // ASCII space is a single byte and never appears inside a multibyte
        // UTF-8 sequence, so byte iteration is safe here and avoids decoding.
        let to_remove = line.bytes().take(width).take_while(|b| *b == b' ').count();
        if to_remove > 0 {
            line.replace_range(0..to_remove, "");
        }
        removed.push(to_remove);
    }
    removed
}

// Comment toggling

pub fn toggle_comment(
    lines: &mut [String],
    first: usize,
    last: usize,
    cursor_line: usize,
    cursor_col: usize,
    prefix: &str,
) -> (usize, usize) {
    if first > last || last >= lines.len() {
        return (cursor_line, cursor_col);
    }

    let all_commented = (first..=last).all(|i| {
        let trimmed = lines.get(i).map_or("", |line| line.trim_start());
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });

    for line in &mut lines[first..=last] {
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

// Case transforms

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_lines_prepends_spaces_to_each_line() {
        let mut lines = vec!["alpha".into(), "beta".into(), "gamma".into()];
        indent_lines(&mut lines, 0, 1, 4);
        assert_eq!(
            lines,
            vec!["    alpha".to_string(), "    beta".into(), "gamma".into()]
        );
    }

    #[test]
    fn indent_lines_indents_empty_lines_too() {
        let mut lines = vec!["a".into(), String::new(), "b".into()];
        indent_lines(&mut lines, 0, 2, 4);
        assert_eq!(
            lines,
            vec!["    a".to_string(), "    ".into(), "    b".into()]
        );
    }

    #[test]
    fn outdent_lines_strips_up_to_width_and_reports_per_line() {
        let mut lines = vec!["      six".into(), "  two".into(), "no_ws".into()];
        let removed = outdent_lines(&mut lines, 0, 2, 4);
        assert_eq!(
            lines,
            vec!["  six".to_string(), "two".into(), "no_ws".into()]
        );
        assert_eq!(removed, vec![4, 2, 0]);
    }

    #[test]
    fn outdent_lines_only_counts_leading_spaces() {
        let mut lines = vec!["\talready".into()];
        let removed = outdent_lines(&mut lines, 0, 0, 4);
        assert_eq!(lines, vec!["\talready".to_string()]);
        assert_eq!(removed, vec![0]);
    }

    #[test]
    fn toggle_comment_round_trips() {
        let mut lines = vec!["fn main() {}".to_string()];
        let (_, col) = toggle_comment(&mut lines, 0, 0, 0, 0, "//");
        assert_eq!(lines, vec!["// fn main() {}".to_string()]);
        assert_eq!(col, 3);
        let _ = toggle_comment(&mut lines, 0, 0, 0, col, "//");
        assert_eq!(lines, vec!["fn main() {}".to_string()]);
    }
}

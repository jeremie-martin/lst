#!/usr/bin/env python3
"""Generate checked-in Vim oracle fixtures from local nvim."""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "crates/lst-editor/tests/fixtures/vim_oracle.json"


def case(
    name: str,
    area: str,
    text: str,
    cursor: tuple[int, int],
    keys: str,
    *,
    requires_surround: bool = False,
) -> dict:
    return {
        "name": name,
        "area": area,
        "initial_text": text,
        "cursor": {"line": cursor[0], "column": cursor[1]},
        "keys": keys,
        "requires_surround": requires_surround,
    }


def build_cases() -> list[dict]:
    cases: list[dict] = []

    motion_text = "  alpha beta gamma\none two three\ncall(foo[bar])"
    for name, cursor, keys in [
        ("motion 0 line start", (0, 10), "0"),
        ("motion first nonblank", (0, 10), "^"),
        ("motion line end", (0, 2), "$"),
        ("motion word forward", (0, 2), "w"),
        ("motion counted word forward", (0, 2), "2w"),
        ("motion word end", (0, 2), "e"),
        ("motion counted word end", (0, 2), "2e"),
        ("motion word backward", (0, 13), "b"),
        ("motion counted word backward", (0, 13), "2b"),
        ("motion big word forward", (0, 2), "W"),
        ("motion big word end", (0, 2), "E"),
        ("motion big word backward", (0, 13), "B"),
        ("motion document start", (2, 8), "gg"),
        ("motion counted gg", (0, 2), "2gg"),
        ("motion document end", (0, 2), "G"),
        ("motion counted G", (0, 2), "2G"),
        ("motion percentage", (0, 2), "50%"),
        ("motion matching paren", (2, 4), "%"),
        ("motion matching bracket", (2, 8), "%"),
    ]:
        cases.append(case(name, "motions", motion_text, cursor, keys))

    char_text = "abc abc abc"
    for name, cursor, keys in [
        ("char find forward", (0, 0), "fc"),
        ("char till forward", (0, 0), "tc"),
        ("char counted find forward", (0, 0), "2fb"),
        ("char find backward", (0, 10), "Fb"),
        ("char till backward", (0, 10), "T "),
        ("char repeat forward", (0, 0), "fc;"),
        ("char reverse repeat", (0, 0), "fc;,"),
    ]:
        cases.append(case(name, "motions", char_text, cursor, keys))

    edit_text = "alpha beta gamma delta"
    motions = [
        ("word", "w"),
        ("word end", "e"),
        ("counted word", "2w"),
        ("line end", "$"),
        ("find char", "fb"),
        ("till char", "tb"),
        ("inner word", "iw"),
        ("a word", "aw"),
    ]
    for motion_name, motion in motions:
        cases.append(case(f"delete {motion_name}", "operators", edit_text, (0, 0), f"d{motion}"))
        cases.append(
            case(
                f"change {motion_name}",
                "operators",
                edit_text,
                (0, 0),
                f"c{motion}X<esc>",
            )
        )
        cases.append(
            case(
                f"yank paste {motion_name}",
                "operators",
                edit_text,
                (0, 0),
                f"y{motion}$p",
            )
        )

    for name, cursor, keys in [
        ("delete backward word", (0, 11), "db"),
        ("change backward word", (0, 11), "cbX<esc>"),
        ("yank backward word paste", (0, 11), "yb$p"),
    ]:
        cases.append(case(name, "operators", "alpha beta gamma", cursor, keys))

    line_text = "one\ntwo\nthree\nfour"
    for name, keys in [
        ("delete current line", "dd"),
        ("delete counted lines", "2dd"),
        ("change current line", "ccX<esc>"),
        ("change counted lines", "2ccX<esc>"),
        ("yank line paste after", "yyGp"),
        ("yank counted lines paste after", "2yyGp"),
        ("delete line motion down", "dj"),
        ("delete line motion to end", "dG"),
        ("change line motion down", "cjX<esc>"),
        ("yank line motion down paste", "yjGp"),
    ]:
        cases.append(case(name, "operators", line_text, (0, 0), keys))

    object_specs = [
        ("quote", 'prefix "alpha beta" tail', (0, 9), '"'),
        ("single quote", "prefix 'alpha beta' tail", (0, 9), "'"),
        ("backtick", "prefix `alpha beta` tail", (0, 9), "`"),
        ("paren", "call(alpha, beta) tail", (0, 7), "("),
        ("bracket", "items[alpha, beta] tail", (0, 8), "["),
        ("brace", "fn { alpha beta } tail", (0, 6), "{"),
        ("angle", "tag<alpha beta> tail", (0, 5), ">"),
    ]
    for object_name, text, cursor, delimiter in object_specs:
        for prefix, label, suffix in [
            ("d", "delete inner", ""),
            ("c", "change inner", "X<esc>"),
            ("y", "yank inner paste", "$p"),
        ]:
            cases.append(
                case(
                    f"{label} {object_name}",
                    "text_objects",
                    text,
                    cursor,
                    f"{prefix}i{delimiter}{suffix}",
                )
            )
        for prefix, label, suffix in [
            ("d", "delete a", ""),
            ("c", "change a", "X<esc>"),
        ]:
            cases.append(
                case(
                    f"{label} {object_name}",
                    "text_objects",
                    text,
                    cursor,
                    f"{prefix}a{delimiter}{suffix}",
                )
            )

    for name, keys in [
        ("append after cursor", "aX<esc>"),
        ("insert before cursor", "iX<esc>"),
        ("insert first nonblank", "IX<esc>"),
        ("append line end", "AX<esc>"),
        ("delete char", "x"),
        ("delete counted chars", "3x"),
        ("delete before cursor", "X"),
        ("substitute char", "sX<esc>"),
        ("substitute counted chars", "3sX<esc>"),
        ("delete to end", "D"),
        ("change to end", "CX<esc>"),
        ("replace char", "rX"),
        ("replace counted chars", "3rX"),
    ]:
        cases.append(case(name, "normal_edits", "  alpha beta", (0, 2), keys))

    for name, text, cursor, keys in [
        ("open line below", "  alpha\nbeta", (0, 2), "oX<esc>"),
        ("open line above", "  alpha\nbeta", (1, 0), "OX<esc>"),
        ("change full line", "  alpha\nbeta", (0, 2), "Snew<esc>"),
        ("join following line", "alpha\n beta", (0, 0), "J"),
        ("join counted lines", "a\n b\n c\nd", (0, 0), "3J"),
        ("join counted lines with final blank", "a\n b\n", (0, 0), "3J"),
        ("indent current line", "alpha\nbeta", (0, 0), ">>"),
        ("indent counted lines", "alpha\nbeta\ngamma", (0, 0), "2>>"),
        ("outdent current line", "  alpha\n  beta", (0, 0), "<<"),
        ("outdent counted lines", "  alpha\n  beta\ngamma", (0, 0), "2<<"),
    ]:
        cases.append(case(name, "normal_edits", text, cursor, keys))

    for name, text, cursor, keys in [
        ("char delete paste after", "alpha beta", (0, 0), "dw$p"),
        ("char delete paste before", "alpha beta", (0, 0), "dwP"),
        ("inner word yank paste after", "alpha beta", (0, 0), "yiw$p"),
        ("line delete paste after", "one\ntwo\nthree", (0, 0), "ddGp"),
        ("line delete paste before", "one\ntwo\nthree", (1, 0), "ddggP"),
        ("empty paste after", "alpha beta", (0, 0), "p"),
        ("empty paste before", "alpha beta", (0, 0), "P"),
    ]:
        cases.append(case(name, "registers", text, cursor, keys))

    for name, text, cursor, keys in [
        ("visual delete inner word", "alpha beta", (0, 0), "viwd"),
        ("visual change inner word", "alpha beta", (0, 0), "viwcX<esc>"),
        ("visual uppercase inner word", "alpha beta", (0, 0), "viwU"),
        ("visual lowercase inner word", "ALPHA beta", (0, 0), "viwu"),
        ("visual delete forward word", "alpha beta gamma", (0, 0), "vwd"),
        ("visual uppercase backward word", "alpha beta gamma", (0, 11), "vbU"),
        ("visual line delete", "alpha\nbeta\ngamma", (0, 0), "Vjd"),
        ("visual line change", "alpha\nbeta\ngamma", (0, 0), "VjcX<esc>"),
        ("visual line indent", "alpha\nbeta\ngamma", (0, 0), "Vj>"),
        ("visual line outdent", "  alpha\n  beta\ngamma", (0, 0), "Vj<"),
    ]:
        cases.append(case(name, "visual", text, cursor, keys))

    for name, text, cursor, keys in [
        ("search forward and repeat", "foo bar foo baz foo", (0, 0), "/foo<enter>n"),
        ("search backward repeat", "foo bar foo baz foo", (0, 16), "/foo<enter>N"),
        ("star search", "foo bar foo baz foo", (0, 0), "*"),
        ("hash search", "foo bar foo baz foo", (0, 16), "#"),
    ]:
        cases.append(case(name, "search", text, cursor, keys))

    for name, text, cursor, keys in [
        ("surround inner word paren", "hello world", (0, 0), "ysiw)"),
        ("surround a word bracket", "hello world", (0, 0), "ysaw]"),
        ("surround to end quote", "hello world", (0, 0), 'ys$"'),
        ("delete paren surround", "(hello)", (0, 1), "ds)"),
        ("change paren to bracket", "(hello)", (0, 1), "cs)]"),
    ]:
        cases.append(case(name, "surround", text, cursor, keys, requires_surround=True))

    return cases


LUA_RUNNER = r'''
local input_path = arg[1]
local output_path = arg[2]

local input = vim.json.decode(table.concat(vim.fn.readfile(input_path), "\n"))

local function split_lines(text)
  if text == "" then
    return { "" }
  end
  local lines = {}
  local start = 1
  while true do
    local idx = string.find(text, "\n", start, true)
    if idx == nil then
      table.insert(lines, string.sub(text, start))
      break
    end
    table.insert(lines, string.sub(text, start, idx - 1))
    start = idx + 1
  end
  return lines
end

local function mode_name(mode)
  if mode == "n" or mode == "no" then
    return "Normal"
  elseif mode == "i" or mode == "ic" or mode == "ix" then
    return "Insert"
  elseif mode == "v" then
    return "Visual"
  elseif mode == "V" then
    return "VisualLine"
  end
  return mode
end

local function has_map(lhs)
  return vim.fn.maparg(lhs, "n") ~= ""
end

local surround_mappings_detected = has_map("ys") and has_map("ds") and has_map("cs")
local version = vim.version()

local out = {
  metadata = {
    generator = input.generator,
    oracle_profile = "user_config",
    nvim_version = string.format("NVIM v%d.%d.%d", version.major, version.minor, version.patch),
    options = {
      selection = vim.o.selection,
      selectmode = vim.o.selectmode,
      virtualedit = vim.o.virtualedit,
      whichwrap = vim.o.whichwrap,
      iskeyword = vim.bo.iskeyword,
      expandtab = vim.bo.expandtab,
      shiftwidth = vim.bo.shiftwidth,
      tabstop = vim.bo.tabstop,
      softtabstop = vim.bo.softtabstop,
    },
    editor_indent_policy = {
      expandtab = true,
      shiftwidth = 2,
      tabstop = 2,
      softtabstop = 2,
    },
    surround_mappings_detected = surround_mappings_detected,
  },
  cases = {},
}

for _, case in ipairs(input.cases) do
  if not case.requires_surround or surround_mappings_detected then
    vim.cmd("silent! %bwipeout!")
    vim.cmd("enew!")
    vim.bo.buftype = "nofile"
    vim.bo.bufhidden = "wipe"
    vim.bo.swapfile = false
    vim.api.nvim_buf_set_lines(0, 0, -1, true, split_lines(case.initial_text))
    vim.api.nvim_win_set_cursor(0, { case.cursor.line + 1, case.cursor.column })
    vim.fn.setreg('"', "")
    vim.fn.setreg("/", "")
    vim.bo.expandtab = true
    vim.bo.shiftwidth = 2
    vim.bo.tabstop = 2
    vim.bo.softtabstop = 2

    local keys = vim.api.nvim_replace_termcodes(case.keys, true, false, true)
    vim.api.nvim_feedkeys(keys, "mx", false)
    vim.cmd("redraw")

    local cursor = vim.api.nvim_win_get_cursor(0)
    local lines = vim.api.nvim_buf_get_lines(0, 0, -1, true)
    table.insert(out.cases, {
      name = case.name,
      area = case.area,
      initial_text = case.initial_text,
      cursor = case.cursor,
      keys = case.keys,
      expected = {
        text = table.concat(lines, "\n"),
        cursor = { line = cursor[1] - 1, column = cursor[2] },
        mode = mode_name(vim.api.nvim_get_mode().mode),
      },
    })
  end
end

vim.fn.writefile(vim.split(vim.json.encode(out), "\n", { plain = true }), output_path)
vim.cmd("qa!")
'''


def generate(output: Path) -> None:
    payload = {
        "generator": "scripts/generate_vim_oracle_fixtures.py",
        "cases": build_cases(),
    }

    with tempfile.TemporaryDirectory() as temp_dir:
        temp = Path(temp_dir)
        input_path = temp / "vim-oracle-input.json"
        output_path = temp / "vim-oracle-output.json"
        lua_path = temp / "vim-oracle.lua"
        input_path.write_text(json.dumps(payload), encoding="utf-8")
        lua_path.write_text(LUA_RUNNER, encoding="utf-8")

        result = subprocess.run(
            [
                "nvim",
                "--headless",
                "-i",
                "NONE",
                "-n",
                "-l",
                str(lua_path),
                str(input_path),
                str(output_path),
            ],
            check=False,
            cwd=ROOT,
            text=True,
            capture_output=True,
        )
        if result.returncode != 0:
            print(result.stdout, end="")
            print(result.stderr, end="")
            result.check_returncode()

        fixture = json.loads(output_path.read_text(encoding="utf-8"))

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(fixture, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {len(fixture['cases'])} vim oracle cases to {output}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"fixture path to write (default: {DEFAULT_OUTPUT})",
    )
    args = parser.parse_args()
    generate(args.output)


if __name__ == "__main__":
    main()

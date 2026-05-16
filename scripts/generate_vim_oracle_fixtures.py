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
    assert_register: bool = False,
    assert_search: bool = False,
    assert_selection: bool = False,
) -> dict:
    return {
        "name": name,
        "area": area,
        "initial_text": text,
        "cursor": {"line": cursor[0], "column": cursor[1]},
        "keys": keys,
        "assert_register": assert_register,
        "assert_search": assert_search,
        "assert_selection": assert_selection,
    }


def key_label(key: str) -> str:
    labels = {
        '"': "double-quote",
        "'": "single-quote",
        "`": "backtick",
        "<lt>": "lt",
        ">": "gt",
        "{": "open-brace",
        "}": "close-brace",
        "[": "open-bracket",
        "]": "close-bracket",
        "(": "open-paren",
        ")": "close-paren",
    }
    return labels.get(key, key)


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

    for name, text, cursor, keys in [
        ("motion counted right clamps at eol", "abc", (0, 0), "9l"),
        ("motion counted left clamps at bol", "abc", (0, 2), "9h"),
        ("motion counted down preserves column", "abcdef\nab\nabcde", (0, 5), "2j"),
        ("motion counted up preserves column", "abcdef\nab\nabcde", (2, 4), "2k"),
        ("motion arrow left", "abc", (0, 2), "<left>"),
        ("motion arrow right", "abc", (0, 0), "<right>"),
        ("motion arrow down", "abcdef\nab", (0, 5), "<down>"),
        ("motion arrow up", "abcdef\nab", (1, 1), "<up>"),
        ("motion home key", "  alpha", (0, 6), "<home>"),
        ("motion end key", "alpha", (0, 0), "<end>"),
        ("motion word through punctuation", "alpha+beta gamma", (0, 0), "3w"),
        ("motion big word through punctuation", "alpha+beta gamma", (0, 0), "W"),
        ("motion word across empty line", "alpha\n\nbeta", (0, 0), "2w"),
        ("motion word end across empty line", "alpha\n\nbeta", (0, 0), "2e"),
        ("motion unmatched percent is noop", "alpha beta", (0, 0), "%"),
        ("motion failed forward find is noop", "abc abc", (0, 0), "fz"),
        ("motion failed backward find is noop", "abc abc", (0, 6), "Fz"),
        ("motion failed repeat after no find is noop", "abc abc", (0, 0), ";"),
        ("motion failed reverse repeat after no find is noop", "abc abc", (0, 0), ","),
    ]:
        cases.append(case(name, "motions", text, cursor, keys))

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
        cases.append(case(f"delete {motion_name}", "operators", edit_text, (0, 0), f"d{motion}", assert_register=True))
        cases.append(
            case(
                f"change {motion_name}",
                "operators",
                edit_text,
                (0, 0),
                f"c{motion}X<esc>",
                assert_register=True,
            )
        )
        cases.append(
            case(
                f"yank paste {motion_name}",
                "operators",
                edit_text,
                (0, 0),
                f"y{motion}$p",
                assert_register=True,
            )
        )

    for name, cursor, keys in [
        ("delete backward word", (0, 11), "db"),
        ("change backward word", (0, 11), "cbX<esc>"),
        ("yank backward word paste", (0, 11), "yb$p"),
    ]:
        cases.append(case(name, "operators", "alpha beta gamma", cursor, keys, assert_register=True))

    object_ops = [
        ("d", "delete", ""),
        ("c", "change", "X<esc>"),
        ("y", "yank paste", "$p"),
    ]
    operator_motion_groups = [
        (
            "punctuation-forward",
            "alpha+beta gamma delta",
            (0, 0),
            [
                ("big word", "W"),
                ("big word end", "E"),
                ("counted big word", "2W"),
                ("counted big word end", "2E"),
                ("find punctuation", "f+"),
                ("till punctuation", "t+"),
            ],
        ),
        (
            "backward",
            "alpha beta gamma delta",
            (0, 16),
            [
                ("word backward", "b"),
                ("big word backward", "B"),
                ("counted word backward", "2b"),
                ("counted big word backward", "2B"),
                ("find backward space", "F "),
                ("till backward space", "T "),
            ],
        ),
        (
            "line-local",
            "  alpha beta gamma",
            (0, 9),
            [
                ("line start first nonblank", "^"),
                ("line end", "$"),
                ("left", "h"),
                ("right", "l"),
                ("counted right", "3l"),
            ],
        ),
    ]
    for group_name, text, cursor, motions_for_group in operator_motion_groups:
        for motion_name, motion in motions_for_group:
            for operator, operator_label, suffix in object_ops:
                cases.append(
                    case(
                        f"{operator_label} {group_name} motion {motion_name}",
                        "operators",
                        text,
                        cursor,
                        f"{operator}{motion}{suffix}",
                        assert_register=True,
                    )
                )

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
        cases.append(case(name, "operators", line_text, (0, 0), keys, assert_register=True))

    linewise_operator_motions = [
        ("down", "j", (0, 0)),
        ("up", "k", (2, 0)),
        ("to document start", "gg", (2, 0)),
        ("counted gg", "2gg", (3, 0)),
        ("to document end", "G", (1, 0)),
        ("counted G", "2G", (0, 0)),
        ("percentage", "50%", (0, 0)),
        ("counted line end", "2$", (0, 0)),
    ]
    for motion_name, motion, cursor in linewise_operator_motions:
        for operator, operator_label, suffix in [
            ("d", "delete", ""),
            ("c", "change", "X<esc>"),
            ("y", "yank paste", "Gp"),
        ]:
            cases.append(
                case(
                    f"{operator_label} linewise motion {motion_name}",
                    "operators",
                    line_text,
                    cursor,
                    f"{operator}{motion}{suffix}",
                    assert_register=True,
                )
            )

    count_text = "one two three four five six seven"
    for motion_name, motion in [
        ("word", "w"),
        ("word end", "e"),
        ("big word", "W"),
        ("big word end", "E"),
    ]:
        for operator_count in [2, 3]:
            for motion_count in [2, 3]:
                for operator, operator_label, suffix in object_ops:
                    cases.append(
                        case(
                            f"{operator_label} operator count {operator_count} motion count {motion_count} {motion_name}",
                            "operators",
                            count_text,
                            (0, 0),
                            f"{operator_count}{operator}{motion_count}{motion}{suffix}",
                            assert_register=True,
                        )
                    )

    backward_count_text = "one two three four five six"
    backward_cursor = (0, len(backward_count_text) - 1)
    for motion_name, motion in [
        ("word backward", "b"),
        ("big word backward", "B"),
    ]:
        for operator_count in [2, 3]:
            for motion_count in [2, 3]:
                for operator, operator_label, suffix in object_ops:
                    cases.append(
                        case(
                            f"{operator_label} backward operator count {operator_count} motion count {motion_count} {motion_name}",
                            "operators",
                            backward_count_text,
                            backward_cursor,
                            f"{operator_count}{operator}{motion_count}{motion}{suffix}",
                            assert_register=True,
                        )
                    )

    line_count_text = "one\ntwo\nthree\nfour\nfive\nsix"
    for motion_name, motion in [("down", "j"), ("line end", "$"), ("document end", "G")]:
        for operator_count in [2, 3]:
            for motion_count in [2, 3]:
                for operator, operator_label, suffix in [
                    ("d", "delete", ""),
                    ("c", "change", "X<esc>"),
                    ("y", "yank paste", "Gp"),
                ]:
                    cases.append(
                        case(
                            f"{operator_label} line operator count {operator_count} motion count {motion_count} {motion_name}",
                            "operators",
                            line_count_text,
                            (0, 0),
                            f"{operator_count}{operator}{motion_count}{motion}{suffix}",
                            assert_register=True,
                        )
                    )

    for name, text, cursor, keys in [
        ("failed forward find preserves last successful find", "abc abc abc", (0, 0), "fc0fz;"),
        ("failed till find preserves last successful find", "abc abc abc", (0, 0), "tc0tz;"),
        ("failed backward find preserves last successful find", "abc abc abc", (0, 8), "Fb$Fz;"),
        ("failed delete find preserves register", "alpha beta gamma", (0, 0), "yiwdfz"),
        ("failed change find preserves register and mode", "alpha beta gamma", (0, 0), "yiwcfz"),
        ("unmatched percent operator preserves register", "alpha beta gamma", (0, 0), "yiwd%"),
        ("missing quote object preserves register", "alpha beta gamma", (0, 0), "yiwdi\""),
        ("missing paren object preserves register", "alpha beta gamma", (0, 0), "yiwdi("),
        ("left at bol operator is noop", "alpha beta", (0, 0), "yiwdh"),
        ("line start at bol operator is noop", "alpha beta", (0, 0), "yiwd0"),
        ("delete from end to start then undo restores document", line_text, (0, 0), "Gdggu"),
    ]:
        cases.append(case(name, "operators", text, cursor, keys, assert_register=True))

    object_specs = [
        {
            "name": "double quote",
            "text": 'prefix "alpha beta" tail',
            "positions": [("open", (0, 7)), ("middle", (0, 9)), ("close", (0, 18))],
            "aliases": ['"'],
        },
        {
            "name": "single quote",
            "text": "prefix 'alpha beta' tail",
            "positions": [("open", (0, 7)), ("middle", (0, 9)), ("close", (0, 18))],
            "aliases": ["'"],
        },
        {
            "name": "backtick",
            "text": "prefix `alpha beta` tail",
            "positions": [("open", (0, 7)), ("middle", (0, 9)), ("close", (0, 18))],
            "aliases": ["`"],
        },
        {
            "name": "paren",
            "text": "call(alpha, beta) tail",
            "positions": [("open", (0, 4)), ("middle", (0, 7)), ("close", (0, 16))],
            "aliases": ["(", ")", "b"],
        },
        {
            "name": "bracket",
            "text": "items[alpha, beta] tail",
            "positions": [("open", (0, 5)), ("middle", (0, 8)), ("close", (0, 17))],
            "aliases": ["[", "]"],
        },
        {
            "name": "brace",
            "text": "fn { alpha beta } tail",
            "positions": [("open", (0, 3)), ("middle", (0, 6)), ("close", (0, 16))],
            "aliases": ["{", "}", "B"],
        },
        {
            "name": "angle",
            "text": "tag<alpha beta> tail",
            "positions": [("open", (0, 3)), ("middle", (0, 5)), ("close", (0, 14))],
            "aliases": ["<lt>", ">"],
        },
    ]
    for spec in object_specs:
        for position_name, cursor in spec["positions"]:
            for alias in spec["aliases"]:
                for prefix, object_label in [("i", "inner"), ("a", "a")]:
                    for operator, operator_label, suffix in object_ops:
                        cases.append(
                            case(
                                f"{operator_label} {object_label} {spec['name']} via {key_label(alias)} at {position_name}",
                                "text_objects",
                                spec["text"],
                                cursor,
                                f"{operator}{prefix}{alias}{suffix}",
                                assert_register=True,
                            )
                        )

    for name, text, cursor, keys in [
        ("change escaped double quote", 'prefix "a\\"b" tail', (0, 10), 'ci"X<esc>'),
        ("delete escaped double quote a-object", 'prefix "a\\"b" tail', (0, 10), 'da"'),
        ("change counted a word", "one two three four", (0, 0), "c2awX<esc>"),
        ("delete counted inner word", "one two three four", (0, 0), "d2iw"),
        ("yank counted a word paste", "one two three four", (0, 0), "y2aw$p"),
        ("change counted a big-word", "one+two three four", (0, 0), "c2aWX<esc>"),
        ("delete counted inner big-word", "one+two three four", (0, 0), "d2iW"),
        ("change inner paragraph", "one\ntwo\n\nthree\nfour", (0, 0), "cipX<esc>"),
        ("delete a paragraph", "one\ntwo\n\nthree\nfour", (0, 0), "dap"),
        ("yank inner paragraph paste", "one\ntwo\n\nthree\nfour", (0, 0), "yipGp"),
    ]:
        cases.append(case(name, "text_objects", text, cursor, keys, assert_register=True))

    counted_object_specs = [
        ("word", "w", "one two three four five", (0, 0)),
        ("big-word", "W", "one+two three four five", (0, 0)),
    ]
    for object_name, object_key, text, cursor in counted_object_specs:
        for prefix, object_label in [("i", "inner"), ("a", "a")]:
            for object_count in [2, 3]:
                for operator, operator_label, suffix in object_ops:
                    cases.append(
                        case(
                            f"{operator_label} counted {object_label} {object_name} count {object_count}",
                            "text_objects",
                            text,
                            cursor,
                            f"{operator}{object_count}{prefix}{object_key}{suffix}",
                            assert_register=True,
                        )
                    )

    for name, text, cursor, keys in [
        ("change empty inner paren inserts inside pair", "()", (0, 1), "ci(X<esc>"),
        ("delete empty a paren removes pair", "()", (0, 1), "da("),
        ("change empty inner bracket inserts inside pair", "[]", (0, 1), "ci]X<esc>"),
        ("delete empty a bracket removes pair", "[]", (0, 1), "da]"),
        ("change nested nearest paren", "outer(inner(value)) tail", (0, 13), "ci(X<esc>"),
        ("delete nested nearest bracket", "items[outer[inner]] tail", (0, 13), "di]"),
        ("quote object missing close is noop", "prefix \"alpha beta tail", (0, 9), "yiwdi\""),
        ("paren object missing close is noop", "call(alpha beta tail", (0, 6), "yiwdi)"),
        ("angle object missing open is noop", "alpha beta> tail", (0, 6), "yiwdi>"),
    ]:
        cases.append(case(name, "text_objects", text, cursor, keys, assert_register=True))

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
        ("delete beyond eol clamps register", "abc", (0, 1), "9x$p"),
        ("delete before bol preserves register", "alpha beta", (0, 0), "yiwX"),
        ("substitute empty line inserts without clobbering register", "", (0, 0), "sX<esc>"),
        ("change to end at eol captures char", "abc", (0, 2), "CX<esc>"),
        ("delete to end at eol captures char", "abc", (0, 2), "D"),
        ("join last line is noop", "alpha\nbeta", (1, 0), "J"),
        ("replace beyond eol is noop", "abc", (0, 1), "9rx"),
        ("indent count clamps at eof", "alpha\nbeta", (0, 0), "9>>"),
        ("outdent count clamps at eof", "  alpha\n  beta", (0, 0), "9<<"),
    ]:
        cases.append(case(name, "normal_edits", text, cursor, keys, assert_register=keys[0] in {"D", "C"} or "x$p" in keys or keys.startswith("yiw")))

    for name, text, cursor, keys in [
        ("char delete paste after", "alpha beta", (0, 0), "dw$p"),
        ("char delete paste before", "alpha beta", (0, 0), "dwP"),
        ("inner word yank paste after", "alpha beta", (0, 0), "yiw$p"),
        ("line delete paste after", "one\ntwo\nthree", (0, 0), "ddGp"),
        ("line delete paste before", "one\ntwo\nthree", (1, 0), "ddggP"),
        ("empty paste after", "alpha beta", (0, 0), "p"),
        ("empty paste before", "alpha beta", (0, 0), "P"),
    ]:
        cases.append(case(name, "registers", text, cursor, keys, assert_register=True))

    for name, text, cursor, keys in [
        ("yank char register exact text", "alpha beta gamma", (0, 0), "yw"),
        ("delete char register exact text", "alpha beta gamma", (0, 0), "dw"),
        ("change char register exact text", "alpha beta gamma", (0, 0), "cwX<esc>"),
        ("yank line register exact text", "one\ntwo\nthree", (0, 0), "yy"),
        ("delete line register exact text", "one\ntwo\nthree", (0, 0), "dd"),
        ("change line register exact text", "one\ntwo\nthree", (0, 0), "ccX<esc>"),
        ("yank counted line register exact text", "one\ntwo\nthree\nfour", (0, 0), "2yy"),
        ("delete counted line register exact text", "one\ntwo\nthree\nfour", (0, 0), "2dd"),
        ("visual yank char register exact text", "alpha beta gamma", (0, 0), "vwy"),
        ("visual delete char register exact text", "alpha beta gamma", (0, 0), "vwd"),
        ("visual line yank register exact text", "one\ntwo\nthree", (0, 0), "Vjy"),
        ("visual line delete register exact text", "one\ntwo\nthree", (0, 0), "Vjd"),
        ("failed find does not clobber char register", "alpha beta gamma", (0, 0), "yiwdfz"),
        ("failed text object does not clobber char register", "alpha beta gamma", (0, 0), "yiwdi\""),
        ("failed empty paste keeps empty register", "alpha beta", (0, 0), "p"),
    ]:
        cases.append(case(name, "registers", text, cursor, keys, assert_register=True))

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
        cases.append(case(name, "visual", text, cursor, keys, assert_register=keys[-1] in {"d", "x", "y"} or "cX<esc>" in keys))

    for spec in object_specs:
        for alias in spec["aliases"]:
            cursor = spec["positions"][1][1]
            for prefix, object_label in [("i", "inner"), ("a", "a")]:
                cases.append(
                    case(
                        f"visual uppercase {object_label} {spec['name']} via {key_label(alias)}",
                        "visual",
                        spec["text"],
                        cursor,
                        f"v{prefix}{alias}U",
                    )
                )
                cases.append(
                    case(
                        f"visual delete {object_label} {spec['name']} via {key_label(alias)}",
                        "visual",
                        spec["text"],
                        cursor,
                        f"v{prefix}{alias}d",
                        assert_register=True,
                    )
                )

    for name, text, cursor, keys, assert_register, assert_selection in [
        ("visual final counted word selection", "alpha beta gamma", (0, 0), "v2w", False, True),
        ("visual final reverse word selection", "alpha beta gamma", (0, 11), "vb", False, True),
        ("visual final line selection", "alpha\nbeta\ngamma", (0, 0), "Vj", False, True),
        ("visual final char toggled from line", "alpha\nbeta\ngamma", (0, 0), "Vjv", False, True),
        ("visual final line toggled from char", "alpha\nbeta\ngamma", (0, 0), "vjV", False, True),
        ("visual yank counted word", "alpha beta gamma delta", (0, 0), "v2wy$p", True, False),
        ("visual delete counted word", "alpha beta gamma delta", (0, 0), "v2wd", True, False),
        ("visual substitute counted word", "alpha beta gamma delta", (0, 0), "v2wsX<esc>", True, False),
        ("visual delete reverse selection", "alpha beta gamma", (0, 11), "vbd", True, False),
        ("visual yank reverse selection", "alpha beta gamma", (0, 11), "vby$p", True, False),
        ("visual uppercase counted a word object", "one two three four", (0, 0), "v2awU", False, False),
        ("visual delete counted a word object", "one two three four", (0, 0), "v2awd", True, False),
        ("visual change counted inner word object", "one two three four", (0, 0), "v2iwcX<esc>", True, False),
        ("visual search final selection", "alpha beta alpha beta", (0, 0), "v/beta<enter>n", False, True),
        ("visual search reverse repeat final selection", "alpha beta alpha beta", (0, 0), "v/beta<enter>nN", False, True),
        ("visual paste char replaces selection and captures overwritten text", "one two three", (0, 0), "yiwwviwp", True, False),
        ("visual Paste char preserves pasted register", "one two three", (0, 0), "yiwwviwP", True, False),
        ("visual paste line replaces selection and captures overwritten line", "one\ntwo\nthree", (0, 0), "yyjVp", True, False),
        ("visual Paste line preserves pasted register", "one\ntwo\nthree", (0, 0), "yyjVP", True, False),
    ]:
        cases.append(
            case(
                name,
                "visual",
                text,
                cursor,
                keys,
                assert_register=assert_register,
                assert_selection=assert_selection,
                assert_search="/beta" in keys,
            )
        )

    for name, text, cursor, keys in [
        ("search forward and repeat", "foo bar foo baz foo", (0, 0), "/foo<enter>n"),
        ("search backward repeat", "foo bar foo baz foo", (0, 16), "/foo<enter>N"),
        ("star search", "foo bar foo baz foo", (0, 0), "*"),
        ("hash search", "foo bar foo baz foo", (0, 16), "#"),
    ]:
        cases.append(case(name, "search", text, cursor, keys, assert_search=keys.startswith("/")))

    for name, text, cursor, keys, assert_search in [
        ("search forward wraps", "foo bar foo", (0, 8), "/foo<enter>n", True),
        ("search backward wraps", "foo bar foo", (0, 0), "/foo<enter>N", True),
        ("question search backward", "foo bar foo baz foo", (0, 16), "?foo<enter>", True),
        ("question search repeat backward", "foo bar foo baz foo", (0, 16), "?foo<enter>n", True),
        ("question search opposite repeat", "foo bar foo baz foo", (0, 16), "?foo<enter>N", True),
        ("search not found keeps cursor", "foo bar baz", (0, 4), "/zzz<enter>", True),
        ("question search not found keeps cursor", "foo bar baz", (0, 4), "?zzz<enter>", True),
        ("search query backspace editing", "foo bar baz", (0, 0), "/baq<bs>r<enter>", True),
        ("search multiline forward", "foo\nbar\nfoo\nbaz", (0, 0), "/foo<enter>n", True),
        ("search multiline backward", "foo\nbar\nfoo\nbaz", (2, 0), "/foo<enter>N", True),
        ("question search multiline backward", "foo\nbar\nfoo\nbaz", (2, 0), "?foo<enter>", True),
        ("search punctuation literal", "a+b a+b axb", (0, 0), "/a+b<enter>n", True),
        ("search repeat without query is noop", "foo bar foo", (0, 0), "n", False),
        ("search reverse repeat without query is noop", "foo bar foo", (0, 0), "N", False),
        ("star search punctuation word", "foo_bar foo-bar foo_bar", (0, 0), "*", False),
        ("hash search punctuation word", "foo_bar foo-bar foo_bar", (0, 16), "#", False),
    ]:
        cases.append(case(name, "search", text, cursor, keys, assert_search=assert_search))

    for name, text, cursor, keys, assert_register in [
        ("undo insert append", "alpha beta", (0, 0), "aX<esc>u", False),
        ("undo delete word", "alpha beta gamma", (0, 0), "dwu", True),
        ("undo change word", "alpha beta gamma", (0, 0), "cwX<esc>u", True),
        ("undo yank paste", "alpha beta gamma", (0, 0), "yw$pu", True),
        ("undo delete line", "one\ntwo\nthree", (0, 0), "ddu", True),
        ("undo change line", "one\ntwo\nthree", (0, 0), "ccX<esc>u", True),
        ("undo visual delete", "alpha beta gamma", (0, 0), "vwdu", True),
        ("undo visual change", "alpha beta gamma", (0, 0), "viwcX<esc>u", True),
        ("undo visual paste", "one two three", (0, 0), "yiwwviwpu", True),
        ("undo substitute", "alpha beta", (0, 0), "sX<esc>u", True),
        ("undo replace", "alpha beta", (0, 0), "rXu", False),
        ("undo join", "alpha\n beta", (0, 0), "Ju", False),
        ("undo indent", "alpha\nbeta", (0, 0), ">>u", False),
        ("undo outdent", "  alpha\nbeta", (0, 0), "<<u", False),
    ]:
        cases.append(case(name, "undo", text, cursor, keys, assert_register=assert_register))

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

local function register_state()
  local regtype = vim.fn.getregtype('"')
  if regtype == "V" then
    return {
      kind = "line",
      text = table.concat(vim.fn.getreg('"', 1, true), "\n"),
    }
  end
  return {
    kind = "char",
    text = vim.fn.getreg('"'),
  }
end

local function visual_selection_text(mode, lines)
  if mode ~= "v" and mode ~= "V" then
    return nil
  end

  local anchor = vim.fn.getpos("v")
  local head = vim.fn.getpos(".")
  local start_line = anchor[2]
  local start_col = anchor[3]
  local end_line = head[2]
  local end_col = head[3]
  if end_line < start_line or (end_line == start_line and end_col < start_col) then
    start_line, end_line = end_line, start_line
    start_col, end_col = end_col, start_col
  end

  if mode == "V" then
    local selected = {}
    for line = start_line, end_line do
      table.insert(selected, lines[line] or "")
    end
    return table.concat(selected, "\n")
  end

  if start_line == end_line then
    return string.sub(lines[start_line] or "", start_col, end_col)
  end

  local selected = { string.sub(lines[start_line] or "", start_col) }
  for line = start_line + 1, end_line - 1 do
    table.insert(selected, lines[line] or "")
  end
  table.insert(selected, string.sub(lines[end_line] or "", 1, end_col))
  return table.concat(selected, "\n")
end

local function visual_state(mode)
  if mode ~= "v" and mode ~= "V" then
    return nil
  end
  local anchor = vim.fn.getpos("v")
  local head = vim.api.nvim_win_get_cursor(0)
  return {
    anchor = { line = anchor[2] - 1, column = anchor[3] - 1 },
    head = { line = head[1] - 1, column = head[2] },
  }
end

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
  },
  cases = {},
}

for _, case in ipairs(input.cases) do
  vim.cmd("silent! %bwipeout!")
  vim.cmd("enew!")
  vim.bo.buftype = "nofile"
  vim.bo.bufhidden = "wipe"
  vim.bo.swapfile = false
  vim.api.nvim_buf_set_lines(0, 0, -1, true, split_lines(case.initial_text))
  vim.cmd("setlocal undolevels=-1")
  vim.cmd("setlocal undolevels=1000")
  vim.api.nvim_win_set_cursor(0, { case.cursor.line + 1, case.cursor.column })
  vim.fn.setreg('"', "")
  vim.fn.setreg("/", "")
  vim.bo.expandtab = true
  vim.bo.shiftwidth = 2
  vim.bo.tabstop = 2
  vim.bo.softtabstop = 2

  local keys = vim.api.nvim_replace_termcodes(case.keys, true, true, true)
  vim.api.nvim_feedkeys(keys, "mx", false)
  vim.cmd("redraw")

  local cursor = vim.api.nvim_win_get_cursor(0)
  local lines = vim.api.nvim_buf_get_lines(0, 0, -1, true)
  local mode = vim.api.nvim_get_mode().mode
  local expected = {
    text = table.concat(lines, "\n"),
    cursor = { line = cursor[1] - 1, column = cursor[2] },
    mode = mode_name(mode),
  }
  if case.assert_register then
    expected.register = register_state()
  end
  if case.assert_search then
    expected.search_query = vim.fn.getreg("/")
  end
  if case.assert_selection then
    expected.selection = visual_selection_text(mode, lines)
    expected.visual_state = visual_state(mode)
  end
  table.insert(out.cases, {
    name = case.name,
    area = case.area,
    initial_text = case.initial_text,
    cursor = case.cursor,
    keys = case.keys,
    expected = expected,
  })
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

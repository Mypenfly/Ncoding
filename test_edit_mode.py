#!/usr/bin/env python3
"""
全面测试 FilesOperator edit 模式的脚本。
覆盖: 基础替换、增量、减量、省略匹配、边界情况、错误处理。
按规范模拟 old_lines 的 whitespace-insensitive 连续匹配以及 new_lines 的缩进继承。
"""

import re
import sys
from pathlib import Path

TEST_DIR = Path("test_edit_workspace")


def setup():
    if TEST_DIR.exists():
        import shutil
        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True)


def teardown():
    if TEST_DIR.exists():
        import shutil
        shutil.rmtree(TEST_DIR
def write_file(rel_path, content):
    fpath = TEST_DIR / rel_path
    fpath.parent.mkdir(parents=True, exist_ok=True)
    fpath.write_text(content, encoding="utf-8")


def read_file(rel_path):
    return (TEST_DIR / rel_path).read_text(encoding="utf-8")


class EditEmulator:
    """模拟 FilesOperator Edit 模式的匹配与缩进继承。"""

    @staticmethod
    def normalize(line):
        return re.sub(r'\s+', '', line)

    @staticmethod
    def indent(line):
        return line[:len(line) - len(line.lstrip())]

    @staticmethod
    def find_match(file_lines, segments):
        norm = [EditEmulator.normalize(l) for l in file_lines]
        matches = []
        pos = 0
        for seg in segments:
            if not seg:
                matches.append((pos, pos))
                continue
            seg_norm = [EditEmulator.normalize(l) for l in seg]
            found = False
            for i in range(pos, len(norm) - len(seg_norm) + 1):
                if norm[i:i + len(seg_norm)] == seg_norm:
                    matches.append((i, i + len(seg_norm)))
                    pos = i + len(seg_norm)
                    found = True
                    break
            if not found:
                return None
        return matches

    @staticmethod
    def apply_edit(original_content, old_lines_raw, new_lines_raw):
        if not original_content:
            original = ['']
        else:
            original = original_content.splitlines(keepends=True)
        if original and not original[-1].endswith('\n'):
            original[-1] += '\n'

        file_lines = [l.rstrip('\n').rstrip('\r') for l in original]

        # 分割 old_lines 为 segments（按 "..."）
        raw_old = old_lines_raw.strip('\n').splitlines()
        segments = []
        cur = []
        for line in raw_old:
            if line.strip() == '...':
                segments.append(cur)
                cur = []
            else:
                cur.append(line)
        segments.append(cur)

        head_anchor = not segments[0]
        tail_anchor = not segments[-1]
        core_segs = [s for s in segments if s]

        if not core_segs:
            return True, new_lines_raw.rstrip('\n') + '\n' if new_lines_raw.strip() else '', None

        matches = EditEmulator.find_match(file_lines, core_segs)
        if matches is None:
            return False, None, "No match found"

        start = 0 if head_anchor else matches[0][0]
        end = len(file_lines) if tail_anchor else matches[-1][1]

        raw_new = new_lines_raw.strip('\n')
        new_list = raw_new.splitlines() if raw_new []

        base_indent = EditEmulator.indent(original[start]) if start < len(original) else ""

        ref_indent = ""
        for nl in new_list:
            if nl.strip() and nl.strip() != '...':
                ref_indent = EditEmulator.indent(nl)
                break

        result_new = []
        for nl in new_list:
            stripped = nl.strip()
            if not stripped:
                result_new.append('\n')
                continue
            if stripped == '...':
                result_new.append(base_indent + '...\n')
                continue
            ni = EditEmulator.indent(nl)
            extra = ni[len(ref_indent):] if ref_indent and ni.startswith(ref_indent) else ni
            result_new.append(base_indent + extra + nl.lstrip() + '\n')

        return True, ''.join(original[:start] + result_new + original[end:]), None


def run_tests():
    setup()
    results = []

    def test(name, content, old, new, expected, fail_ok=False):
        write_file(f"t_{len(results.txt", content)
        original = read_file(f"t_{len(results)}.txt")
        ok, result, err = EditEmulator.apply_edit(original, old, new)
        if fail_ok:
            results.append((not ok, name, f"Expected fail, got: {err}" if ok else f"OK: {err}"))
            return
        if not ok:
            results.append((False, name, f"Edit fail: {err}"))
            return
        rn = result.rstrip('\n')
        en = expected.rstrip('\n')
        results.append((rn == en, name,
                        "PASS" if rn == en else f"MISMATCH\n  Want: {repr(en)}\n  Got:  {repr(rn)}"))

    # ------------------------------------------------------------------
    test("1: 基础单行替换",
         "line1\nline2\nline3\n",
         "line2", "modified_line2",
         "line1\nmodified_line2\nline3\n")

    test("2: 增量 1→3 缩进继承",
         "def foo():\n    old_code()\n    return x\n",
         "    old_code()",
         "    new_a()\n        nested()\n    new_b()",
         "def foo():\n    new_a()\n        nested()\n    new_b()\n    return x\n")

    test("3: 减量 3→1",
         "def bar():\n    step1()\n    step2()\n   3()\n    return y\n",
         "    step1()\n    step2()\n    step3()",
         "    all_steps()",
         "def bar():\n    all_steps()\n    return y\n")

    test("4: ... 中间省略",
         "class C:\n    def f(self):\n        a=1\n        b=2\n        c=3\n    def g(self): pass\n",
         "    def f(self):\n        ...\n        c=3",
         "    def f(self):\n        ...\n        c=99",
         "class C:\n    def f(self):\n        a=1\n        b=2\n        c=99\n    def g(self): pass\n")

    test("5: 多段 ...",
         "def f(a,b):\n    if a<0:\n        raise\n    r=a+b\n    if r>100:\n        return 100\n    return r\n",
         "def f(a,b):\n    ...\n    if a<0:\n        ...\n    r=a+b\n    ...\n    return r",
         "def f(a,b):\n    ...\n    if a<0:\n        ...\n    r=a*b\n    ...\n    return r",
         "def f(a,b):\n    if a<0:\n        raise\n    r=a*b\n    if r>100:\n        return 100\n    return r\n")

    test("6: 空 new_lines 删除行",
         "keep\nremove\nkeep2\n",
         "remove", "",
         "keep\nkeep2\n")

    test("7: 匹配失败",
         "a\nb\nc\n",
         "x", "y", "", fail_ok=True)

    test("8: 复杂缩进继承",
         "outer:\n    mid:\n        inner()\n        inner2()\n    end\n",
         "        inner()\n        inner2()",
         "        while 1:\n            work()\n            break",
         "outer:\n    mid:\n        while 1:\n            work()\n            break\n    end\n")

    test("9: ... 开头+中间替换",
         "import os\n# hello\nx=1\ny=2\n# bye\n",
         "...\nx=1\ny=2",
         "...\nx=99\ny=99",
         "import os\n# hello\nx=99\ny=99\n# bye\n")

    test("10: 空行匹配",
         "def a(): pass\n\ndef b(): pass\n",
         "\ndef b():",
         "\n# sep\ndef b():",
         "def a(): pass\n# sep\ndef b(): pass\n")

    test("11: Unicode 替换",
         "print('你好')\nprint('こんにちは')\n",
         "print('こんにちは')",
         "print('hello')",
         "print('你好')\nprint('hello')\n")

    test("12: for → comprehension 重构",
         "def p(d):\n    c=[]\n    for i in d:\n        if i>0:\n            c.append(i)\n    return c\n",
         "    c=[]\n    for i in d:\n        if i>0:\n            c.append(i)",
         "    c=[i for i in d if i>0]",
         "def p(d):\n    c=[i for i in d if i>0]\n    return c\n")

    test("13: 重复行歧义(第一组)",
         "x=1\ny=1\nx=1\nz=2\n",
         "x=1\ny=1", "a=99\nb=99",
         "a=99\nb=99\nx=1\nz=2\n")

    test("14: 文件末尾替换",
         "head\nmid\nfoot\n",
         "foot", "new_foot",
         "head\nmid\nnew_foot\n")

    test("15: 函数体中间 ...",
         "def f():\n    s1()\n    s2()\n    s3()\n\nother\n",
         "def f():\n    ...\n    s3()",
         "def f():\n    ...\n    fin()",
         "def f():\n    s1()\n    s2()\n    fin()\n\nother\n")

    test("16: 首尾 ... 中间保留",
         "import sys\n\nprint(1)\n# end\n",
         "...\nprint(1)\n...",
         "...\nprint(999)\n...",
         "import sys\n\nprint(999)\n# end\n")

    test("17: 相对缩进保持",
         "if cond:\n    act1()\nelse:\n    act2()\n",
         "if cond:\n    act1()",
         "if cond:\n    pre()\n        inner()\n    post()",
         "if cond:\n    pre()\n        inner()\n    post()\nelse:\n    act2()\n")

    test("18: 一行→多行 纯增量",
         "a\nplace\nc\n",
         "place", "x\ny",
         "a\nx\ny\nc\n")

    test("19: 跨空行注释 ...",
         "# Start\n\n# Mid\na=1\n\n# End\nb=2\n",
         "# Start\n...\n# End",
         "# Start\n...\n# Finish",
         "# Start\n\n# Mid\na=1\n\n# Finish\nb=2\n")

    test("20: 特殊字符 * → #",
         "tab:\t\tx\nstar: ***\nplain\n",
         "star: ***", "hash: ###",
         "tab:\t\tx\nhash: ###\nplain\n")

    test("21: 无缩进替换",
         "A\nB\nC\n",
         "B", "X",
         "A\nX\nC\n")

    test("22: new_lines 中的 ... 字面保留",
         "def f():\n    old\n    return\n",
         "def f():\n    old",
         "def f():\n    ...\n    new",
         "def f():\n    ...\n    new\n    return\n")

    test("23: ... 在 old 末尾",
         "h\na\nb\nc\n",
         "a\n...", "a\n...\nz",
         "h\na\nb\nc\nz\n")

    test("24: 空行精确匹配",
         "def a(): pass\n\ndef b(): pass\n",
         "def a(): pass\n\ndef b(): pass",
         "def a(): pass\n\n# ---\ndef b(): pass",
         "def a(): pass\n# ---\ndef b(): pass\n")

    test("25: 大幅缩进变化",
         "def o():\n        deep()\n        d2()\n    mid\n",
         "        deep()\n        d2()",
         "            deeper()\n                more()\n            back()",
         "def o():\n            deeper()\n                more()\n            back()\n    mid\n")

    test("26: 全文件替换(...)",
         "old_all\n",
         "...", "new",
         "new\n")

    test("27: ... 在开头",
         "preamble\nmatch\nrest\n",
         "...\nmatch", "...\nnew_match",
         "preamble\nnew_match\nrest\n")

    test("28: new 完全为空字符串",
         "a\nb\nc\n",
         "b", "",
         "a\nc\n")

    # ------------------------------------------------------------------
    passed = sum(1 for r in results if r[0])
    failed = sum(1 for r in results if not r[0])

    print("\n" + "=" * 60)
    print("FILESOPERATOR EDIT MODE — TEST RESULTS")
    print("=" * 60    for i, (ok, name, detail) in enumerate(results):
        tag = "PASS" if ok else "FAIL"
        print(f"\n[{tag}] {i+1}: {name}")
        if not ok:
            print(f"       {detail}")

    print("\n" + "=" * 60)
    print(f"TOTAL {len(results)}  |  PASS {passed}  |  FAIL {failed}")
    print("=" * 60)

    teardown()
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(run_tests())

#!/usr/bin/env python3
"""tools/todo_audit.py - TODO audit report generator"""
import os, re, sys, json
from datetime import datetime
from collections import defaultdict

# Comment markers and their regex
TODO_RE = re.compile(r'(?:#|//|--|<!--|;)\s*(TODO)\s*[:：]?\s*(.*)', re.IGNORECASE)
FIXME_RE = re.compile(r'(?:#|//|--|<!--|;)\s*(FIXME)\s*[:：]?\s*(.*)', re.IGNORECASE)
HACK_RE = re.compile(r'(?:#|//|--|<!--|;)\s*(HACK)\s*[:：]?\s*(.*)', re.IGNORECASE)
XXX_RE = re.compile(r'(?:#|//|--|<!--|;)\s*(XXX)\s*[:：]?\s*(.*)', re.IGNORECASE)
BUG_RE = re.compile(r'(?:#|//|--|<!--|;)\s*(BUG)\s*[:：]?\s*(.*)', re.IGNORECASE)

PATTERNS = {'TODO': TODO_RE, 'FIXME': FIXME_RE, 'HACK': HACK_RE, 'XXX': XXX_RE, 'BUG': BUG_RE}
SKIP_DIRS = {'.git', '__pycache__', 'node_modules', '.npm'}


def scan(root):
    results = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for fn in filenames:
            try:
                text = open(os.path.join(dirpath, fn), errors='ignore').read()
            except:
                continue
            rel = os.path.relpath(os.path.join(dirpath, fn), root)
            for tag, pat in PATTERNS.items():
                for m in pat.finditer(text):
                    results.append({
                        'file': rel,
                        'line': text[:m.start()].count('\n') + 1,
                        'tag': tag,
                        'text': m.group(2).strip() or '(no description)',
                    })
    return results


def format_text(results):
    by = defaultdict(list)
    for r in results:
        by[r['tag']].append(r)

    lines = [
        '=' * 60,
        'TODO 审计报告',
        f'生成时间: {datetime.now().strftime("%Y-%m-%d %H:%M")}',
        f'总计: {len(results)} 个标记',
        '=' * 60,
    ]

    for tag in ['TODO', 'FIXME', 'BUG', 'HACK', 'XXX']:
        items = by.get(tag, [])
        if not items:
            continue
        lines.append(f'\n[{tag}] {len(items)} 个')
        for r in items[:15]:
            lines.append(f'  {r["file"]}:{r["line"]}  {r["text"][:60]}')
        if len(items) > 15:
            lines.append(f'  ...还有 {len(items) - 15} 个')

    # File distribution
    from collections import Counter
    file_counts = Counter(r['file'] for r in results)
    lines.append(f'\n文件分布 (前15):')
    for f, c in file_counts.most_common(15):
        lines.append(f'  {c:3d}  {f}')

    return '\n'.join(lines)


if __name__ == '__main__':
    import argparse
    p = argparse.ArgumentParser(description='TODO audit report generator')
    p.add_argument('root', nargs='?', default='.')
    p.add_argument('--json', action='store_true')
    p.add_argument('-o', '--output')
    args = p.parse_args()

    root = os.path.abspath(args.root)
    results = scan(root)

    if args.json:
        output = json.dumps({
            'total': len(results),
            'by_tag': {k: len([r for r in results if r['tag'] == k]) for k in PATTERNS},
            'items': results,
        }, indent=2)
    else:
        output = format_text(results)

    if args.output:
        with open(args.output, 'w') as f:
            f.write(output)
        print(f"报告已保存: {args.output}")
    else:
        print(output)

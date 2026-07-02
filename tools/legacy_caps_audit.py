#!/usr/bin/env python3
"""
tools/legacy_caps_audit.py — LEGACY uppercase enforcement audit
Bounty: #2 ($6)
Scans the repository for files containing "legacy" (case-insensitive)
and verifies each has an uppercase "LEGACY" comment marker.
"""

import os, sys, re, argparse

# 要跳过的文件（二进制、构建产物、第三方库）
SKIP_PATTERNS = [
    r'\.git/',
    r'__pycache__',
    r'\.pyc$',
    r'\.tsbuildinfo$',
    r'node_modules/',
    r'\.gitignore$',
    r'Cargo\.lock$',
    r'package-lock\.json$',
    r'go\.sum$',
]


def should_skip(filepath):
    """判断是否应该跳过该文件"""
    rel = os.path.normpath(filepath).replace('\\', '/')
    for pat in SKIP_PATTERNS:
        if re.search(pat, rel):
            return True
    return False


def scan_repo(root_dir):
    """扫描仓库，返回所有legacy文件及其LEGACY状态"""
    results = []
    for dirpath, dirnames, filenames in os.walk(root_dir):
        # 跳过.git
        if '.git' in dirpath:
            continue
        for fn in filenames:
            fpath = os.path.join(dirpath, fn)
            if should_skip(fpath):
                continue
            try:
                with open(fpath, 'r', errors='ignore') as f:
                    content = f.read()
            except:
                continue

            if 'legacy' not in content.lower():
                continue

            has_legacy_comment = bool(re.search(r'#\s*LEGACY|//\s*LEGACY|--\s*LEGACY|<!--\s*LEGACY|/\*\s*LEGACY', content))
            rel = os.path.relpath(fpath, root_dir).replace('\\', '/')

            results.append({
                'path': rel,
                'has_LEGACY': has_legacy_comment,
            })
    return results


def auto_fix(results, root_dir):
    """对缺少LEGACY注释的文件，添加注释"""
    fixed = 0
    for r in results:
        if r['has_LEGACY']:
            continue
        fpath = os.path.join(root_dir, r['path'].replace('/', os.sep))
        try:
            with open(fpath, 'r', errors='ignore') as f:
                content = f.read()
        except:
            continue

        # 判断文件类型，决定注释风格
        ext = os.path.splitext(r['path'])[1].lower()
        if ext in ('.rs', '.go', '.c', '.cpp', '.h', '.hpp', '.js', '.ts', '.tsx', '.jsx', '.css', '.scss', '.less', '.java'):
            comment = '// LEGACY: contains legacy code\n'
        elif ext in ('.py', '.rb', '.sh', '.yaml', '.yml', '.pl'):
            comment = '# LEGACY: contains legacy code\n'
        elif ext == '.lua':
            comment = '-- LEGACY: contains legacy code\n'
        elif ext in ('.md', '.txt'):
            comment = '<!-- LEGACY: contains legacy code -->\n'
        elif ext == '.sql':
            comment = '-- LEGACY: contains legacy code\n'
        elif ext == '.hs':
            comment = '-- LEGACY: contains legacy code\n'
        elif ext in ('.html', '.htm', '.xml'):
            comment = '<!-- LEGACY: contains legacy code -->\n'
        else:
            comment = '# LEGACY: contains legacy code\n'

        # 添加到文件顶部的注释区
        # 如果文件以 #!/ 开头，在 shebang 之后添加
        lines = content.split('\n')
        if lines and lines[0].startswith('#!'):
            lines.insert(1, comment.rstrip())
        else:
            lines.insert(0, comment.rstrip())

        with open(fpath, 'w') as f:
            f.write('\n'.join(lines))
        r['has_LEGACY'] = True
        fixed += 1
        print(f"  ✅ 修复: {r['path']}")

    return fixed


def main():
    parser = argparse.ArgumentParser(description='LEGACY uppercase enforcement audit')
    parser.add_argument('root_dir', nargs='?', default='.',
                        help='Repository root directory')
    parser.add_argument('--fix', action='store_true',
                        help='Auto-fix violations by adding LEGACY comments')
    parser.add_argument('--json', action='store_true',
                        help='Output as JSON')
    args = parser.parse_args()

    root = os.path.abspath(args.root_dir)
    print(f"🔍 扫描仓库: {root}")
    print()

    results = scan_repo(root)

    total = len(results)
    violations = [r for r in results if not r['has_LEGACY']]
    compliant = [r for r in results if r['has_LEGACY']]

    print(f"  含legacy的文件: {total}")
    print(f"  合规(有LEGACY): {len(compliant)}")
    print(f"  违规(缺LEGACY): {len(violations)}")
    print()

    if violations:
        print("违规文件:")
        for r in violations:
            print(f"  ❌ {r['path']}")

    if args.fix and violations:
        print(f"\n自动修复 {len(violations)} 个违规...")
        fixed = auto_fix(violations, root)
        print(f"\n✅ 修复完成: {fixed}/{len(violations)}")

        # 验证
        print("\n验证修复结果...")
        results2 = scan_repo(root)
        violations2 = [r for r in results2 if not r['has_LEGACY']]
        if violations2:
            print(f"⚠️ 仍有 {len(violations2)} 个违规未修复:")
            for r in violations2:
                print(f"  ❌ {r['path']}")
        else:
            print("✅ 全部修复成功!")

    if args.json:
        import json
        print(json.dumps({'total': total, 'compliant': len(compliant),
                         'violations': len(violations), 'files': results}, indent=2))

    return 0 if not violations else 1


if __name__ == '__main__':
    sys.exit(main())
